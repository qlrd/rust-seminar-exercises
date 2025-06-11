use crate::error::SeminarNodeError;
use crate::peer::Peer;
use crate::peer::PeerStatus;

use bitcoin::consensus::encode::{Decodable, Encodable};
use bitcoin::network::Network;
use bitcoin::p2p::ServiceFlags;
use bitcoin::p2p::address::Address;
use bitcoin::p2p::message::{NetworkMessage, RawNetworkMessage};
use bitcoin::p2p::message_blockdata::Inventory;
use bitcoin::p2p::message_network::VersionMessage;

use log::{debug, info, warn};

use rand::seq::IteratorRandom;

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::sync::mpsc::Sender;
use tokio::sync::mpsc::channel;
use tokio::time::timeout;

use std::collections::VecDeque;

// Maintains a buffer for stream decoding
struct ConnectionBuffer {
    buffer: VecDeque<u8>,
}

impl ConnectionBuffer {
    fn new() -> Self {
        Self {
            buffer: VecDeque::with_capacity(8192),
        }
    }

    fn append(&mut self, data: &[u8]) {
        self.buffer.extend(data);
    }

    fn try_decode(&mut self) -> Option<RawNetworkMessage> {
        let buf = self.buffer.make_contiguous();
        let mut cursor = std::io::Cursor::new(buf);

        match RawNetworkMessage::consensus_decode(&mut cursor) {
            Ok(msg) => {
                let read = cursor.position() as usize;
                self.buffer.drain(..read);
                Some(msg)
            }
            Err(_) => None,
        }
    }
}

#[derive(Debug)]
/// SeminarNode is a simple implementation of a
/// Bitcoin node that connects to a peer-to-peer
/// network, following the tasks outlined in the seminar.
pub struct SeminarNode {
    /// This is used by bitcoin::network::message::VersionMessage
    version: u32,

    /// The address of the transmitting node as perceived by the receiving node.
    /// This is used by bitcoin::network::message::Address
    addr_trans: Address,

    /// This is used by bitcoin::network::Networknet, etc.)
    /// Valid values are
    /// bitcoin::network::Network::Bitcoin,
    /// bitcoin::network::Network::Testnet,
    /// bitcoin::network::Network::Regtest
    /// bitcoin::network::Network::Signet
    network: Network,

    /// The starting height of the node.
    /// This is used by bitcoin::network::message::VersionMessage
    start_height: i32,

    /// Whether the node should relay transactions.
    /// This is used by bitcoin::network::message::VersionMessage
    relay: bool,

    /// The user agent of the node.
    /// This is used by bitcoin::network::message::VersionMessage
    user_agent: String,

    /// The database connection to the peers table
    peers_db_path: String,

    in_use_peers: Arc<Mutex<HashSet<(String, u16)>>>,

    /// peer count is the number of made connections
    peer_count: u32,

    /// The maximum number of peers to connect to.
    max_peers: u32,
}

impl SeminarNode {
    /// Create a new SeminarNode instance
    /// with the given IP address, version, and network.
    ///
    /// Our client have own user agent called
    /// "RustSeminar:0.0.1/rust:(1.87.0-nightly-3f5502370)/"
    /// following BIP14
    ///
    /// Once the node receive the 'addr' message,
    /// it will try to connect to more peers until reached
    /// the max_peers allowed by the node (the default is 8).
    ///
    /// ```rust
    /// use seminar_node::SeminarNode;
    /// use seminar_node::SeminarNodeError;
    /// fn main() -> Result<(), SeminarNodeError> {
    ///     let ip = "123.456.789.0";
    ///     let port = 8333;
    ///     let max_peers = 8u16;
    ///     let relay = false;
    ///     let mut node = SeminarNode::create(ip.to_string(), 8333, max_peers, relay)?;
    ///     node.run()
    /// }
    /// ```
    pub fn create(
        host: String,
        port: u16,
        max_peers: u32,
        relay: bool,
        peers_db_path: String,
    ) -> Result<Self, SeminarNodeError> {
        // Configure connection to localhost and to peer
        let me = Ipv4Addr::new(127u8, 0u8, 0u8, 1u8);
        let localhost = me.to_ipv6_mapped();

        // Configure version, addressecv/trans services
        let version = 70016;
        info!("Version: {version}");

        let addr_trans = Address {
            services: ServiceFlags::NONE,
            address: localhost.segments(),
            port: 8333,
        };

        // Bip 14 user agent
        let user_agent = "/RustSeminar:0.0.4/".to_string();
        info!("User agent: {user_agent}");

        // Create a table of peers if not exist
        let path = Path::new(&peers_db_path);
        info!("Database: {path:?}/peers.db");
        Peer::create_table(path).map_err(SeminarNodeError::CreateDatabaseError)?;

        // Add the provided host peer to the database
        // it will me used as fallback if no peers are found
        if let Err(err) = Peer::create_peer(path, host.to_string(), port) {
            if let SeminarNodeError::PeerAlreadyExists(_, _) = err {
                warn!("Peer {host}:{port} already exists in the database");
            } else {
                return Err(err);
            }
        }

        Ok(Self {
            version,
            addr_trans,
            network: Network::Bitcoin,
            start_height: 0,
            relay,
            user_agent,
            peers_db_path,
            in_use_peers: Arc::new(Mutex::new(HashSet::new())),
            peer_count: 0,
            max_peers,
        })
    }

    /// Choose a random peer from the Database filtered by a given condition.
    fn choose_random_peer_from_database<T>(&self, filter: T) -> Result<Peer, SeminarNodeError>
    where
        T: Fn(&Peer) -> bool,
    {
        debug!("Choosing a random peer from the database with filter");
        let path = Path::new(&self.peers_db_path);
        let all_peers = Peer::get_all_peers(path).map_err(SeminarNodeError::SelectPeerError)?;

        if all_peers.is_empty() {
            return Err(SeminarNodeError::NoPeersFound);
        }

        let mut rng = rand::rng();

        if let Some(peer) = all_peers
            .iter()
            .filter(|p| p.status == PeerStatus::Awaiting && filter(p))
            .choose(&mut rng)
        {
            debug!("Selected {peer:?}");
            return Ok(peer.clone());
        }

        Err(SeminarNodeError::NoPeersFound)
    }

    /// Create a socket address from the peer's IP and port.
    fn create_socket_addr(&self, peer: &Peer) -> Result<(SocketAddr, Address), SeminarNodeError> {
        debug!("Creating socket address for peer: {peer:?}");
        let socket_ip = Ipv4Addr::from_str(&peer.ip).map_err(SeminarNodeError::MalformedString)?;
        let socket_ipv6 = socket_ip.to_ipv6_mapped();
        let socket = SocketAddr::new(IpAddr::V6(socket_ipv6), peer.port);

        let addr_recv = Address {
            services: ServiceFlags::NONE,
            address: socket_ipv6.segments(),
            port: peer.port,
        };

        Ok((socket, addr_recv))
    }

    /// Connect to the many nodes
    /// using tokio::net::TcpStream with an tokio::net::SocketAddr
    /// and make a handshake.
    pub async fn run(&mut self) -> Result<(), SeminarNodeError> {
        let max = self.get_max_peers()?;
        debug!("Running with max_peers: {max}");
        let (tx, mut rx) = channel::<(TcpStream, Address)>(max as usize);

        // Multi-producer
        self.run_workers(max, tx)?;

        // Single consumer
        while let Some((mut stream, addr_recv)) = rx.recv().await {
            let mut node = self.clone_for_worker();

            tokio::spawn(async move {
                let _ = node.handle_peer(&mut stream, addr_recv.clone()).await;
            });
        }
        Ok(())
    }

    /// Run the worker to connect to peers.
    /// It will spawn `max` workers that will try to connect to
    /// random peers from the database.
    fn run_workers(
        &mut self,
        max: u32,
        tx: Sender<(TcpStream, Address)>,
    ) -> Result<(), SeminarNodeError> {
        debug!("Running worker to connect to peers");
        let timeout_duration = Duration::from_secs(30);

        for _ in 0..max {
            let tx = tx.clone();
            let this = self.clone_for_worker();
            let in_use_peers = Arc::clone(&self.in_use_peers);

            tokio::spawn(async move {
                loop {
                    let mut guard = in_use_peers.lock().await;
                    if guard.len() >= this.max_peers as usize {
                        warn!("Max peers in use, waiting for a slot");
                        tokio::time::sleep(timeout_duration).await;
                        continue;
                    }

                    let peer = match this.choose_random_peer_from_database(|p| {
                        !guard.contains(&(p.ip.clone(), p.port))
                    }) {
                        Ok(p) => {
                            guard.insert((p.ip.clone(), p.port));
                            p
                        }
                        Err(_) => {
                            tokio::time::sleep(timeout_duration).await;
                            continue;
                        }
                    };

                    let (socket, addr_recv) = match this.create_socket_addr(&peer) {
                        Ok(pair) => pair,
                        Err(_) => {
                            guard.retain(|(ip, port)| ip != &peer.ip && *port != peer.port);
                            tokio::time::sleep(timeout_duration).await;
                            continue;
                        }
                    };

                    let stream = match this.maybe_open_connection(socket).await {
                        Ok(s) => s,
                        Err(_) => {
                            guard.retain(|(ip, port)| ip != &peer.ip && *port != peer.port);
                            tokio::time::sleep(timeout_duration).await;
                            continue;
                        }
                    };

                    if let Err(err) = tx.send((stream, addr_recv)).await {
                        guard.retain(|(ip, port)| ip != &peer.ip && *port != peer.port);
                        warn!("Failed to send peer to receiver: {err:?}");
                        return;
                    }
                }
            });
        }

        Ok(())
    }

    fn get_max_peers(&self) -> Result<u32, SeminarNodeError> {
        let len_peers = Peer::count(Path::new(&self.peers_db_path))
            .map_err(SeminarNodeError::SelectPeerError)?;

        if len_peers == 0 {
            return Err(SeminarNodeError::NoPeersAvailable);
        }

        let max = if len_peers < self.max_peers {
            len_peers
        } else {
            self.max_peers
        };

        Ok(max)
    }

    fn clone_for_worker(&self) -> SeminarNode {
        SeminarNode {
            version: self.version,
            addr_trans: self.addr_trans.clone(),
            network: self.network,
            start_height: self.start_height,
            relay: self.relay,
            user_agent: self.user_agent.clone(),
            peers_db_path: self.peers_db_path.clone(),
            in_use_peers: self.in_use_peers.clone(),
            peer_count: self.peer_count,
            max_peers: self.max_peers,
        }
    }

    /// Try open a connection to the peer
    ///
    /// If the connection is successful, let's do the following:
    /// - if insert is true:
    ///   - if list of active peers is less than max_peers, update to Active status;
    ///   - if list of active peers is equal or greater than max_peers, update to Deactivated;
    /// - if insert is false, update the peer status to Deactivated status
    ///
    /// If the connection fails, update the peer status to Unreachable
    pub async fn maybe_open_connection(
        &self,
        socket: SocketAddr,
    ) -> Result<TcpStream, SeminarNodeError> {
        debug!("Attempting to connect to peer {socket:?}");

        let peer_stream = match TcpStream::connect(socket).await {
            Ok(stream) => {
                debug!("Connected to peer {socket:?}");
                stream
            }
            Err(err) => {
                warn!("Failed to connect to peer {socket:?}: {err:?}");
                let db_path = Path::new(&self.peers_db_path);
                let _ = Peer::update_peer(
                    db_path,
                    socket.ip().to_string(),
                    socket.port(),
                    PeerStatus::Unreachable,
                );
                return Err(SeminarNodeError::ConnectionError(err));
            }
        };

        let db_path = Path::new(&self.peers_db_path);
        let count = Peer::count(db_path).map_err(SeminarNodeError::SelectPeerError)?;
        if count >= self.max_peers {
            let _ = Peer::update_peer(
                db_path,
                socket.ip().to_string(),
                socket.port(),
                PeerStatus::Awaiting,
            );
            debug!("Max peers reached, peer marked Deactivated");
            return Err(SeminarNodeError::MaxPeersReached);
        }

        Peer::update_peer(
            db_path,
            socket.ip().to_string(),
            socket.port(),
            PeerStatus::Reachable,
        )
        .map_err(SeminarNodeError::UpdatePeerError)?;

        Ok(peer_stream)
    }

    fn create_version_message(
        &self,
        addr_recv: Address,
    ) -> Result<VersionMessage, SeminarNodeError> {
        let time_now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(SeminarNodeError::TimeError)?
            .as_secs() as i64;

        Ok(VersionMessage {
            version: self.version,
            services: ServiceFlags::NONE,
            timestamp: time_now,
            receiver: addr_recv.clone(),
            sender: self.addr_trans.clone(),
            nonce: 0,
            user_agent: self.user_agent.clone(),
            start_height: self.start_height,
            relay: self.relay,
        })
    }

    /// Corrected handshake logic in handle_peer per Bitcoin P2P
    /// Node initiates by sending `version`, then waits for peer's `version` and `verack`
    /// Only after both are received, it sends its own `verack`
    async fn handle_peer(
        &mut self,
        stream: &mut TcpStream,
        addr_recv: Address,
    ) -> Result<(), SeminarNodeError> {
        debug!("Handling peer with address: {addr_recv:?}");

        let version = self.create_version_message(addr_recv.clone())?;
        self.send_message(stream, NetworkMessage::Version(version))
            .await?;

        let mut received_version = false;
        let mut received_verack = false;
        let mut conn_buf = ConnectionBuffer::new();

        loop {
            let (cmd, message) = self.receive_message(stream, &mut conn_buf).await?;
            match message {
                NetworkMessage::Version(v) => {
                    received_version = true;
                }
                NetworkMessage::Verack => {
                    received_verack = true;
                }
                _ => warn!("Ignoring {cmd} during handshake: {message:?}"),
            }

            if received_version && received_verack {
                break;
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        self.send_message(stream, NetworkMessage::Verack).await?;
        self.send_message(stream, NetworkMessage::GetAddr).await?;

        loop {
            let (_, message) = self.receive_message(stream, &mut conn_buf).await?;
            self.handle_message(stream, message, addr_recv.clone())
                .await?;
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
    /// Send a bitcoin::p2p::message::NetworkMessage to the node
    /// with the given std::net::TcpStream. After sent, it will
    /// wait for 1 second before doing other things.
    async fn send_message(
        &self,
        stream: &mut TcpStream,
        payload: NetworkMessage,
    ) -> Result<(), SeminarNodeError> {
        let raw_message = RawNetworkMessage::new(self.network.magic(), payload);
        debug!("Sending {raw_message:?}");
        // Serialize
        let mut buffer = Vec::new();
        raw_message
            .consensus_encode(&mut buffer)
            .map_err(SeminarNodeError::EncodeError)?;

        // Send it over the stream
        stream
            .write_all(&buffer)
            .await
            .map_err(SeminarNodeError::SendError)?;

        tokio::time::sleep(Duration::from_secs(1)).await;

        let cmd = raw_message.cmd();
        debug!("Sent {cmd} : {buffer:02x?}");

        Ok(())
    }

    /// Receive a single message from the stream with buffering support
    async fn receive_message(
        &self,
        stream: &mut TcpStream,
        conn_buf: &mut ConnectionBuffer,
    ) -> Result<(String, NetworkMessage), SeminarNodeError> {
        loop {
            if let Some(msg) = conn_buf.try_decode() {
                let cmd = msg.cmd().to_string();
                let payload = msg.payload();
                debug!("Received {cmd} : {payload:?}");
                return Ok((cmd, payload.clone()));
            }

            let mut temp_buf = [0u8; 4096];
            let n = stream
                .read(&mut temp_buf)
                .await
                .map_err(SeminarNodeError::ConnectionError)?;

            if n == 0 {
                return Err(SeminarNodeError::ZeroBytesRecvError);
            }

            conn_buf.append(&temp_buf[..n]);
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn handle_message(
        &mut self,
        stream: &mut TcpStream,
        message: NetworkMessage,
        addr_recv: Address,
    ) -> Result<(), SeminarNodeError> {
        let addr = stream
            .peer_addr()
            .map_err(SeminarNodeError::ConnectionError)?;
        let ip = addr.ip().to_string();
        let port = addr.port();

        info!("Received '{}' message from {ip}:{port}", message.cmd());
        match message {
            NetworkMessage::Version(_) => {
                let version = self.create_version_message(addr_recv)?;
                self.send_message(stream, NetworkMessage::Version(version))
                    .await?;
            }
            NetworkMessage::Verack => {
                self.send_message(stream, NetworkMessage::GetAddr).await?;
            }
            NetworkMessage::Inv(msg) => {
                let n = &msg.len();
                info!("Received {n} inventory items");
                for inv in msg {
                    match inv {
                        Inventory::Transaction(tx) => {
                            info!("Received tx: {tx:?}");
                        }
                        Inventory::Block(block) => {
                            info!("Received block: {block:?}");
                        }
                        _ => {
                            warn!("Received unknown inventory item: {inv:?}");
                        }
                    }
                }
            }
            NetworkMessage::Addr(msg) => {
                let n = &msg.len();
                info!("Received 'addr' message with {n} addresses");

                let db_path = Path::new(&self.peers_db_path);
                let count = Peer::count(db_path).map_err(SeminarNodeError::SelectPeerError)?;
                if count > 100 {
                    warn!("Peer list is too long, ignoring addr message");
                } else {
                    debug!("Handling addr message with {} addresses", msg.len());

                    let addresses = msg.to_vec();
                    for (_, addr) in addresses {
                        let ip = {
                            let ip_segments = addr.address;
                            let ipv6 = Ipv6Addr::new(
                                ip_segments[0],
                                ip_segments[1],
                                ip_segments[2],
                                ip_segments[3],
                                ip_segments[4],
                                ip_segments[5],
                                ip_segments[6],
                                ip_segments[7],
                            );
                            IpAddr::V6(ipv6)
                        };

                        // Try to create a new peer onto the database as an
                        // awaiting peer if it doesn't exist yet.
                        // If the create is successful,
                        // try to connect to the peer and try to add it to the
                        // active peers list. Otherwise, update the peer status
                        // to unreachable.
                        let db_path = Path::new(&self.peers_db_path);
                        Peer::create_peer(db_path, ip.to_string(), addr.port)?;
                    }
                }
            }
            NetworkMessage::Ping(msg) => {
                // Respond the ping
                let payload = NetworkMessage::Pong(msg);
                self.send_message(stream, payload).await?;
            }
            NetworkMessage::WtxidRelay
            | NetworkMessage::SendHeaders
            | NetworkMessage::SendCmpct { .. }
            | NetworkMessage::SendAddrV2 => {
                debug!("Optional capability during handshake: {}", message.cmd());
            }
            _ => warn!("Unexpected message during handshake: {}", message.cmd()),
        }

        // Set read timeout to 30 seconds
        // and send a ping message
        // to keep the connection alive
        let mut buffer = vec![0u8; 1024];
        let result = timeout(Duration::from_secs(30), stream.read(&mut buffer)).await;
        match result {
            Ok(Ok(0)) => Err(SeminarNodeError::ConnectionClosed),
            Ok(Ok(_)) => self.send_message(stream, NetworkMessage::Ping(0)).await,
            Ok(Err(err)) => Err(SeminarNodeError::ConnectionError(err)),
            Err(_) => Err(SeminarNodeError::TimeoutError),
        }
    }
}

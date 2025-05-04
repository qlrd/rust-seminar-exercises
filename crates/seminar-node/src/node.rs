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
use std::collections::HashMap;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
/// SeminarNode is a simple implementation of a
/// Bitcoin node that connects to a peer-to-peer
/// network, following the tasks outlined in the seminar.
pub struct SeminarNode {
    /// The socket address of the node.
    /// This will be used by std::net::TcpStream
    socket: SocketAddr,

    /// The version of the node.
    /// This is used by bitcoin::network::message::VersionMessage
    version: u32,

    /// The address of the receiving node as perceived by the transmitting node.
    /// This is used by bitcoin::network::message::Address
    addr_recv: Address,

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

    /// The list of peers to connect to.
    peers: Arc<Mutex<HashMap<u32, Arc<Mutex<TcpStream>>>>>,

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
        let socket_ip = Ipv4Addr::from_str(&host).map_err(SeminarNodeError::MalformedString)?;
        let socket_ipv6 = socket_ip.to_ipv6_mapped();
        let socket = SocketAddr::new(IpAddr::V6(socket_ipv6), port);

        // Configure version, address, recv/trans services
        let version = 70015;
        let addr_recv = Address {
            services: ServiceFlags::NONE,
            address: socket_ipv6.segments(),
            port,
        };
        let addr_trans = Address {
            services: ServiceFlags::NONE,
            address: localhost.segments(),
            port: 8333,
        };

        // Bip 14 user agent
        let user_agent = String::from("RustSeminar:0.0.3/rust:(rustc 1.88.0");

        Ok(Self {
            socket,
            version,
            addr_recv,
            addr_trans,
            network: Network::Bitcoin,
            start_height: 0,
            relay,
            user_agent,
            peers_db_path,
            peers: Arc::new(Mutex::new(HashMap::new())),
            max_peers,
        })
    }

    /// Connect to the node
    /// using std::net::TcpStream with an std::net::SocketAddr
    /// and make a handshake.
    pub fn run(&mut self) -> Result<(), SeminarNodeError> {
        info!("Attempting to connect with {}", self.socket.ip());

        // Add the peer to the database
        let path = Path::new(&self.peers_db_path);
        Peer::create_table(path).map_err(SeminarNodeError::CreateDatabaseError)?;
        warn!("Peer database in {}/peers.db", path.display());

        match Peer::create_peer(path, self.socket.ip().to_string(), self.socket.port()) {
            Ok(id) => info!("Peer {id} added to the database"),
            Err(e) => warn!("{e:?}"),
        }

        // If connection is successful, update the peer to the peers list
        self.open_connection(self.socket, true)?;

        // Main thread can sleep forever or later start maintenance tasks
        loop {
            thread::sleep(Duration::from_secs(60));
        }
    }

    /// Send a getaddr message to the peer and receive addr messages
    /// from the peer. For each addr message received, try to connect
    /// to the address and add it to the peers list.
    ///
    /// If the connection fails, log the error.
    ///
    /// Another messages aren't handled yet, just log them with a warning.
    fn run_on_peer(&mut self, stream: &mut TcpStream) -> Result<(), SeminarNodeError> {
        // Send getaddr message
        let payload = NetworkMessage::GetAddr;
        self.send_message(stream, payload)?;

        // wait a little
        thread::sleep(Duration::from_secs(1));

        // Receive addr message
        let start_time = SystemTime::now();
        let duration = Duration::new(60, 0); // 1 minute
        while SystemTime::now()
            .duration_since(start_time)
            .map_err(SeminarNodeError::TimeError)?
            < duration
        {
            let received = self.receive_message(stream)?;
            self.handle_message(stream, received.1)?;
        }
        Ok(())
    }

    /// Send a bitcoin::p2p::message::NetworkMessage to the node
    /// with the given std::net::TcpStream. After sent, it will
    /// wait for 1 second before doing other things.
    fn send_message(
        &self,
        stream: &mut TcpStream,
        payload: NetworkMessage,
    ) -> Result<(), SeminarNodeError> {
        let raw_message = RawNetworkMessage::new(self.network.magic(), payload);

        // Serialize
        let mut buffer = Vec::new();
        raw_message
            .consensus_encode(&mut buffer)
            .map_err(SeminarNodeError::EncodeError)?;

        // Send it over the stream
        stream
            .write_all(&buffer)
            .map_err(SeminarNodeError::SendError)?;

        let cmd = raw_message.cmd();
        debug!("Sent {cmd} : {buffer:02x?}");
        thread::sleep(Duration::from_secs(1));

        Ok(())
    }

    /// Receive a message from the node, parse it
    /// with bitcoin::p2p::message::RawNetworkMessage::consensus_decode
    /// and return the command and payload. It will wait for 1 second
    /// before doing other things.
    fn receive_message(
        &self,
        stream: &mut TcpStream,
    ) -> Result<(String, NetworkMessage), SeminarNodeError> {
        // Parse full RawNetworkMessage from the stream
        let message =
            RawNetworkMessage::consensus_decode(stream).map_err(SeminarNodeError::DecodeError)?;

        let cmd = message.cmd().to_string();
        let result = message.payload();

        thread::sleep(Duration::from_secs(1));

        Ok((cmd, result.clone()))
    }

    /// Try open a connection to the peer
    ///
    /// If the connection is successful, let's do the following:
    /// - if insert is true:
    ///   - if list of active peers is less than max_peers, update to Active status;
    ///   - if list of active peers is equal or greater than max_peers, update to Deactivated;
    /// - if insert is false, update the peer status to Deactivated status
    /// If the connection fails, update the peer status to Unreachable
    fn open_connection(&self, socket: SocketAddr, insert: bool) -> Result<(), SeminarNodeError> {
        match TcpStream::connect(socket) {
            Ok(peer_stream) => {
                // Let's send a ping message to the peer
                // and wait for a pong message
                if insert {
                    let arc_peer = Arc::new(Mutex::new(peer_stream));
                    let path = Path::new(&self.peers_db_path);
                    let mut peers = self
                        .peers
                        .lock()
                        .map_err(|_| SeminarNodeError::MaxPeersReached)?;
                    let peer_count = peers.len() as u32;
                    if peer_count >= self.max_peers {
                        Peer::update_peer(
                            path,
                            socket.ip().to_string(),
                            socket.port(),
                            PeerStatus::Deactivated,
                        )
                        .map_err(SeminarNodeError::InsertPeerError)?;
                        return Err(SeminarNodeError::MaxPeersReached);
                    } else {
                        Peer::update_peer(
                            path,
                            socket.ip().to_string(),
                            socket.port(),
                            PeerStatus::Active,
                        )
                        .map_err(SeminarNodeError::InsertPeerError)?;
                        peers.insert(socket.port() as u32, arc_peer.clone());
                    }
                    // Spawn a thread for this peer
                    let mut node = self.clone(); // We'll implement Clone for SeminarNode
                    thread::spawn(move || {
                        if let Ok(mut stream) = arc_peer.lock() {
                            if let Err(e) = node.handle_peer(&mut stream) {
                                warn!("Peer thread ended with error: {e:?}");
                            }
                        }
                    });
                } else {
                    let db_path = Path::new(&self.peers_db_path);
                    Peer::update_peer(
                        db_path,
                        socket.ip().to_string(),
                        socket.port(),
                        PeerStatus::Deactivated,
                    )
                    .map_err(SeminarNodeError::InsertPeerError)?;
                }
            }
            Err(_) => {
                let db_path = Path::new(&self.peers_db_path);
                Peer::update_peer(
                    db_path,
                    socket.ip().to_string(),
                    socket.port(),
                    PeerStatus::Unreachable,
                )
                .map_err(SeminarNodeError::InsertPeerError)?;
            }
        }
        Ok(())
    }

    /// Connect to the node using std::net::TcpStream
    /// and make a handshake.
    fn handle_peer(&mut self, stream: &mut TcpStream) -> Result<(), SeminarNodeError> {
        let time_now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(SeminarNodeError::TimeError)?
            .as_secs() as i64;

        let version = VersionMessage {
            version: self.version,
            services: ServiceFlags::NONE,
            timestamp: time_now,
            receiver: self.addr_recv.clone(),
            sender: self.addr_trans.clone(),
            nonce: 0,
            user_agent: self.user_agent.clone(),
            start_height: self.start_height,
            relay: self.relay,
        };

        self.send_message(stream, NetworkMessage::Version(version))?;

        let mut received_version = false;
        let mut received_verack = false;

        while !(received_version && received_verack) {
            let (_, msg) = self.receive_message(stream)?;
            match msg {
                NetworkMessage::Version(peer_version) => {
                    info!("Received version message: {peer_version:?}");
                    received_version = true;
                }
                NetworkMessage::Verack => {
                    received_verack = true;
                }
                _ => {
                    warn!("Expected version/verack, got: {msg:?}");
                }
            }
        }

        self.send_message(stream, NetworkMessage::Verack)?;

        // Handshake complete
        self.run_on_peer(stream)
    }
    fn handle_message(
        &mut self,
        stream: &mut TcpStream,
        message: NetworkMessage,
    ) -> Result<(), SeminarNodeError> {
        match message {
            NetworkMessage::Inv(msg) => {
                let n = &msg.len();
                info!("Received 'inv' message with {n} inventory items");
                for inv in msg {
                    match inv {
                        Inventory::Transaction(tx) => {
                            info!("Received transaction: {tx:?}");
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
                    self.handle_addr_message(msg)?;
                }
            }
            NetworkMessage::Ping(msg) => {
                // Respond the ping
                let payload = NetworkMessage::Pong(msg);
                self.send_message(stream, payload)?;
            }
            _ => {}
        }

        // Set read timeout to 30 seconds
        // and send a ping message
        // to keep the connection alive
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .map(|_| {
                let payload = NetworkMessage::Ping(0);
                self.send_message(stream, payload)
            })
            .map_err(SeminarNodeError::ConnectionError)?
    }

    fn handle_addr_message(&mut self, msg: Vec<(u32, Address)>) -> Result<(), SeminarNodeError> {
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
            let socket = SocketAddr::new(ip, addr.port);
            self.open_connection(socket, true)?;
        }

        Ok(())
    }
}

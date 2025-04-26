use bitcoin::consensus::encode::{Decodable, Encodable};
use bitcoin::network::Network;
use bitcoin::p2p::ServiceFlags;
use bitcoin::p2p::address::Address;
use bitcoin::p2p::message::{NetworkMessage, RawNetworkMessage};
use bitcoin::p2p::message_network::VersionMessage;
use log::{debug, info, warn};
use std::io::{Error, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub enum SeminarNodeError {
    MalformedString(std::net::AddrParseError),
    ConnectionError(Error),
    EncodeError(bitcoin::io::Error),
    DecodeError(bitcoin::consensus::encode::Error),
    SendError(Error),
    TimeError(std::time::SystemTimeError),
    HandshakeError(String),
    PingPongError(String),
    MaxPeersReached,
    PeerLockError(String),
    NoPeersAvailable,
}

impl std::fmt::Display for SeminarNodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SeminarNodeError::MalformedString(err) => write!(f, "Malformed string: {err}"),
            SeminarNodeError::ConnectionError(err) => write!(f, "Connection error: {err}"),
            SeminarNodeError::EncodeError(err) => write!(f, "Encode error: {err}"),
            SeminarNodeError::DecodeError(err) => write!(f, "Decode error: {err}"),
            SeminarNodeError::SendError(err) => write!(f, "Send error: {err}"),
            SeminarNodeError::TimeError(err) => write!(f, "Time error: {err}"),
            SeminarNodeError::HandshakeError(err) => write!(f, "Handshake error: {err}"),
            SeminarNodeError::PingPongError(err) => write!(f, "PingPong error: {err}"),
            SeminarNodeError::MaxPeersReached => write!(f, "Max peers reached"),
            SeminarNodeError::PeerLockError(err) => write!(f, "Lock error: {err}"),
            SeminarNodeError::NoPeersAvailable => write!(f, "No peers available"),
        }
    }
}

impl Clone for SeminarNode {
    fn clone(&self) -> Self {
        SeminarNode {
            socket: self.socket,
            version: self.version,
            addr_recv: self.addr_recv.clone(),
            addr_trans: self.addr_trans.clone(),
            network: self.network,
            start_height: self.start_height,
            relay: self.relay,
            user_agent: self.user_agent.clone(),
            peers: self.peers.clone(),
            max_peers: self.max_peers,
        }
    }
}

#[derive(Debug)]
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

    /// The list of peers to connect to.
    peers: Arc<Mutex<Vec<Arc<Mutex<TcpStream>>>>>,

    /// The maximum number of peers to connect to.
    max_peers: u16,
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
        max_peers: u16,
        relay: bool,
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

        //
        let user_agent = String::from("RustSeminar:0.0.2/rust:(rustc 1.88.0-nightly-df35ff6c3/");

        Ok(Self {
            socket,
            version,
            addr_recv,
            addr_trans,
            network: Network::Bitcoin,
            start_height: 0,
            relay,
            user_agent,
            peers: Arc::new(Mutex::new(vec![])),
            max_peers,
        })
    }

    /// Connect to the node
    /// using std::net::TcpStream with an std::net::SocketAddr
    /// and make a handshake.
    pub fn run(&mut self) -> Result<(), SeminarNodeError> {
        info!("Attempting to connect with {}", self.socket.ip());

        let stream = TcpStream::connect(self.socket).map_err(SeminarNodeError::ConnectionError)?;

        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .map_err(SeminarNodeError::ConnectionError)?;

        self.add_peer(stream)?;

        // Main thread can sleep forever or later start maintenance tasks
        loop {
            thread::sleep(Duration::from_secs(60));
        }
    }

    /// Add a peer to the node
    ///
    /// Each peer is represented by a TcpStream
    /// and is wrapped in an Arc<Mutex<TcpStream>>
    /// and will be added to the peers list until reached the max_peers
    /// allowed by the node (the default is 8).
    pub fn add_peer(&mut self, stream: TcpStream) -> Result<(), SeminarNodeError> {
        let arc_peer = Arc::new(Mutex::new(stream));

        {
            let mut peers = self
                .peers
                .lock()
                .map_err(|_| SeminarNodeError::MaxPeersReached)?;

            if peers.len() < self.max_peers.into() {
                peers.push(arc_peer.clone());
            } else {
                return Err(SeminarNodeError::MaxPeersReached);
            }
        } // unlock peers early

        // Spawn a thread for this peer
        let mut node = self.clone(); // We'll implement Clone for SeminarNode
        thread::spawn(move || {
            if let Ok(mut stream) = arc_peer.lock() {
                if let Err(e) = node.handle_peer(&mut stream) {
                    warn!("Peer thread ended with error: {e:?}");
                }
            }
        });

        Ok(())
    }

    /// Connect to the node using std::net::TcpStream
    /// and make a handshake.
    pub fn handle_peer(&mut self, stream: &mut TcpStream) -> Result<(), SeminarNodeError> {
        self.handshake(stream)?;
        self.run_on_peer(stream)
    }

    /// Send a bitcoin::p2p::message::NetworkMessage to the node
    /// with the given std::net::TcpStream. After sent, it will
    /// wait for 1 second before doing other things.
    pub fn send_message(
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

        info!("Sent message: {}", raw_message.cmd());
        debug!("Sent hex: {buffer:02x?}");
        info!("wait for 1 second after '{}'", raw_message.cmd());
        thread::sleep(Duration::from_secs(1));

        Ok(())
    }

    /// Receive a message from the node, parse it
    /// with bitcoin::p2p::message::RawNetworkMessage::consensus_decode
    /// and return the command and payload. It will wait for 1 second
    /// before doing other things.
    pub fn receive_message(
        &self,
        stream: &mut TcpStream,
    ) -> Result<(String, NetworkMessage), SeminarNodeError> {
        // Parse full RawNetworkMessage from the stream
        let message =
            RawNetworkMessage::consensus_decode(stream).map_err(SeminarNodeError::DecodeError)?;

        let cmd = message.cmd().to_string();
        let result = message.payload();

        info!("wait for 1 second after '{cmd}'");
        thread::sleep(Duration::from_secs(1));

        Ok((cmd, result.clone()))
    }

    /// Start the handshake process with another bitcoind node
    ///
    /// * A sends to B a version message;
    /// * B responds to A with a version message;
    /// * B responds to A with a verack message;
    /// * A sends to B a verack message;
    ///
    /// After the handshake, B will send to A 3 messages:
    /// * sendcmpct
    /// * ping
    /// * feefilter
    ///
    /// See: https://developer.bitcoin.org/reference/p2p_networking.html#version
    pub fn handshake(&self, stream: &mut TcpStream) -> Result<(), SeminarNodeError> {
        // Prepare version payload
        // see more at https://developer.bitcoin.org/reference/p2p_networking.html#version
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

        // Send version message
        let payload = NetworkMessage::Version(version);
        self.send_message(stream, payload)?;

        // wait a little
        info!("wait for 1 second");
        thread::sleep(Duration::from_secs(1));

        // Receive version message
        match self.receive_message(stream)? {
            (cmd, NetworkMessage::Version(version)) => {
                info!("Received '{cmd}' message: {version:?}");
            }
            (cmd, _) => return Err(SeminarNodeError::HandshakeError(cmd)),
        }

        // receive verack message
        match self.receive_message(stream)? {
            (_, NetworkMessage::Verack) => {
                info!("Received verack message");
            }
            (cmd, _) => return Err(SeminarNodeError::HandshakeError(cmd)),
        }

        // send verack message
        let payload = NetworkMessage::Verack;
        self.send_message(stream, payload)?;

        // After bitcoind receive verack from other peer,
        // it will send to us 3 messages. Receive them until
        // the last is received and then you can do whatever
        // you want
        loop {
            match self.receive_message(stream)? {
                (cmd, NetworkMessage::SendCmpct(msg)) => {
                    info!("Received '{cmd}' message: {msg:?}");
                }
                (cmd, NetworkMessage::Ping(msg)) => {
                    info!("Received '{cmd}' message: {msg:?}");

                    // Respond the ping
                    let payload = NetworkMessage::Pong(msg);
                    self.send_message(stream, payload)?;
                }
                (cmd, NetworkMessage::FeeFilter(msg)) => {
                    info!("Received '{cmd}' message: {msg:?}");
                    break;
                }
                (cmd, _) => {
                    return Err(SeminarNodeError::HandshakeError(cmd));
                }
            }
        }

        info!("Handshake complete.");
        Ok(())
    }

    /// Send a getaddr message to the peer and receive addr messages
    /// from the peer. For each addr message received, try to connect
    /// to the address and add it to the peers list.
    ///
    /// If the connection fails, log the error.
    ///
    /// Another messages aren't handled yet, just log them with a warning.
    pub fn run_on_peer(&mut self, stream: &mut TcpStream) -> Result<(), SeminarNodeError> {
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
            match self.receive_message(stream)? {
                (cmd, NetworkMessage::Addr(msg)) => {
                    let addresses = msg.to_vec();
                    info!(
                        "Received '{}' message with {:?} addresses",
                        cmd,
                        &addresses.len()
                    );
                    for (i, addr) in addresses {
                        debug!(" {i:?}: {addr:#?}");
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
                            if let Some(ipv4) = ipv6.to_ipv4() {
                                IpAddr::V4(ipv4)
                            } else {
                                IpAddr::V6(ipv6)
                            }
                        };

                        let socket = SocketAddr::new(ip, addr.port);
                        info!("Trying to connect to {socket}");

                        match TcpStream::connect(socket) {
                            Ok(peer_stream) => {
                                info!("Connected to {socket}");
                                self.add_peer(peer_stream)?;
                            }
                            Err(e) => {
                                warn!("Failed to connect to {socket}: {e}");
                            }
                        }
                    }
                }
                (cmd, _) => {
                    warn!("Received unhandled message '{cmd}'");
                }
            }
        }
        Ok(())
    }
}

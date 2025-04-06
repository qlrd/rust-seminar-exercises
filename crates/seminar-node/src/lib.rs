use bitcoin::consensus::encode::{Decodable, Encodable};
use bitcoin::network::Network;
use bitcoin::p2p::ServiceFlags;
use bitcoin::p2p::address::Address;
use bitcoin::p2p::message::{NetworkMessage, RawNetworkMessage};
use bitcoin::p2p::message_network::VersionMessage;
use log::{debug, info};
use std::io::{Error, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::str::FromStr;
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
}

impl std::fmt::Display for SeminarNodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SeminarNodeError::MalformedString(err) => write!(f, "Malformed string: {}", err),
            SeminarNodeError::ConnectionError(err) => write!(f, "Connection error: {}", err),
            SeminarNodeError::EncodeError(err) => write!(f, "Encode error: {}", err),
            SeminarNodeError::DecodeError(err) => write!(f, "Decode error: {}", err),
            SeminarNodeError::SendError(err) => write!(f, "Send error: {}", err),
            SeminarNodeError::TimeError(err) => write!(f, "Time error: {}", err),
            SeminarNodeError::HandshakeError(err) => write!(f, "Handshake error: {}", err),
            SeminarNodeError::PingPongError(err) => write!(f, "PingPong error: {}", err),
        }
    }
}

#[derive(Debug)]
/// SeminarNode is a simple implementation of a
/// Bitcoin node that connects to a peer-to-peer
/// network, following the tasks outlined in the seminar.
pub struct SeminarNode {
    socket: SocketAddr,
    version: u32,
    addr_recv: Address,
    addr_trans: Address,
    network: Network,
    start_height: i32,
    relay: bool,
    user_agent: String,
}

impl SeminarNode {
    /// Create a new SeminarNode instance
    /// with the given IP address, version, and network.
    pub fn create(ip: String, port: u16) -> Result<Self, SeminarNodeError> {
        // Configure connection to localhost and to peer
        let me = Ipv4Addr::new(127u8, 0u8, 0u8, 1u8);
        let localhost = me.to_ipv6_mapped();
        let socket_ip = Ipv4Addr::from_str(&ip).map_err(SeminarNodeError::MalformedString)?;
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

        let user_agent = String::from("RustSeminar:0.0.1/rust:(1.87.0-nightly-3f5502370)/");

        Ok(Self {
            socket,
            version,
            addr_recv,
            addr_trans,
            network: Network::Bitcoin,
            start_height: 0,
            relay: false,
            user_agent,
        })
    }

    /// Connect to the node
    pub fn connect(&self) -> Result<TcpStream, SeminarNodeError> {
        info!("Connecting to node at {}", self.socket.ip());
        TcpStream::connect(self.socket).map_err(SeminarNodeError::ConnectionError)
    }

    /// Send version message to the node
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
        info!("Sending version message: {}", raw_message.cmd());
        debug!("Serialized version message (hex): {:02x?}", buffer);
        stream
            .write_all(&buffer)
            .map_err(SeminarNodeError::SendError)?;

        Ok(())
    }

    /// Receive a message from the node, parse and print it
    pub fn receive_message(
        &self,
        stream: &mut TcpStream,
    ) -> Result<(String, NetworkMessage), SeminarNodeError> {
        // Parse full RawNetworkMessage from the stream
        let message =
            RawNetworkMessage::consensus_decode(stream).map_err(SeminarNodeError::DecodeError)?;

        let cmd = message.cmd().to_string();
        let result = message.payload();

        Ok((cmd, result.clone()))
    }

    /// Start the handshake process with another bitcoind node
    ///
    /// * A sends to B a version message;
    /// * B responds to A with a version message;
    /// * B responds to A with a verack message;
    /// * A sends to B a verack message;
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
                info!("Received '{}' message: {:?}", cmd, version);
            }
            (cmd, _) => return Err(SeminarNodeError::HandshakeError(cmd)),
        }

        // wait a little
        info!("wait for 1 second");
        thread::sleep(Duration::from_secs(1));

        // receive verack message
        match self.receive_message(stream)? {
            (_, NetworkMessage::Verack) => {
                info!("Received verack message");
            }
            (cmd, _) => return Err(SeminarNodeError::HandshakeError(cmd)),
        }

        // wait a little
        info!("wait for 1 second");
        thread::sleep(Duration::from_secs(1));

        // send verack message
        let payload = NetworkMessage::Verack;
        self.send_message(stream, payload)?;
        info!("Sent verack message. Handshake complete.");

        // After bitcoind receive verack from other peer,
        // it will send to us 3 messages:
        //
        // * sendcmpctping
        // * ping,
        // * feefilter
        //
        // receive them until the last is received
        // and then you can do whatever you want
        loop {
            match self.receive_message(stream)? {
                (cmd, NetworkMessage::SendCmpct(msg)) => {
                    info!("Received '{}' message: {:?}", cmd, msg);
                }
                (cmd, NetworkMessage::Ping(msg)) => {
                    info!("Received '{}' message: {:?}", cmd, msg);
                }
                (cmd, NetworkMessage::FeeFilter(msg)) => {
                    info!("Received '{}' message: {:?}", cmd, msg);
                    break;
                }
                (cmd, _) => {
                    return Err(SeminarNodeError::HandshakeError(cmd));
                }
            }
        }
        Ok(())
    }
}

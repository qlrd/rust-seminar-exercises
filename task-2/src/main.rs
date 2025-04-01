use byteorder::{BigEndian, LittleEndian, WriteBytesExt};
use hex::encode;
use rand::Rng;
use sha2::{Digest, Sha256};
use std::io::{Error, Read, Write};
use std::net::{IpAddr, TcpStream};
use std::time::{SystemTime, UNIX_EPOCH};

/// Bitcoin compactSize uint format
pub fn compact_size(n: u64) -> Vec<u8> {
    let mut result = Vec::new();
    if n < 0xFD {
        result.push(n as u8);
    } else if n <= 0xFFFF {
        result.push(0xFD);
        result.write_u16::<LittleEndian>(n as u16).unwrap();
    } else if n <= 0xFFFF_FFFF {
        result.push(0xFE);
        result.write_u32::<LittleEndian>(n as u32).unwrap();
    } else {
        result.push(0xFF);
        result.write_u64::<LittleEndian>(n).unwrap();
    }
    result
}

#[derive(Debug)]
enum NodeError {
    ConnectionError(Error),
    SendError(Error),
    ReceiveError(Error),
    WriteError(Error),
    ReadError(Error),
    PayloadError(Error),
}

impl std::fmt::Display for NodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeError::ConnectionError(err) => write!(f, "Connection error: {}", err),
            NodeError::SendError(err) => write!(f, "Send error: {}", err),
            NodeError::ReceiveError(err) => write!(f, "Receive error: {}", err),
            NodeError::WriteError(err) => write!(f, "Write error: {}", err),
            NodeError::ReadError(err) => write!(f, "Read error: {}", err),
            NodeError::PayloadError(err) => write!(f, "Payload error: {}", err),
        }
    }
}

#[derive(Debug)]
enum NodeServices {
    None,
}

impl NodeServices {
    fn as_node(service: NodeServices) -> u64 {
        match service {
            NodeServices::None => 0x00,
        }
    }
}

#[derive(Debug)]
struct NodeAddress<'a>(u64, &'a str, u16);

#[derive(Debug)]
enum MagicBytes {
    Mainnet,
}

impl MagicBytes {
    fn as_bytes(&self) -> &'static [u8; 4] {
        match self {
            MagicBytes::Mainnet => &[0xF9, 0xBE, 0xB4, 0xD9],
        }
    }
}

#[derive(Debug)]
enum Command {
    Version,
}

impl Command {
    fn as_bytes(&self) -> &'static [u8; 12] {
        match self {
            Command::Version => b"version\0\0\0\0\0",
        }
    }
}

#[derive(Debug)]
struct Node<'a> {
    ip: &'a str,
    version: i32,
    addr_recv: NodeAddress<'a>,
    addr_trans: NodeAddress<'a>,
    magic_bytes: MagicBytes,
    start_height: i32,
    relay: bool,
}

impl<'a> Node<'a> {
    fn new(
        ip: &'a str,
        version: i32,
        addr_recv: NodeAddress<'a>,
        addr_trans: NodeAddress<'a>,
        magic_bytes: MagicBytes,
        start_height: i32,
        relay: bool,
    ) -> Self {
        Node {
            ip,
            version,
            addr_recv,
            addr_trans,
            magic_bytes,
            start_height,
            relay,
        }
    }

    fn create(ip: &'a str) -> Self {
        Node::new(
            ip,
            70015,
            NodeAddress(
                NodeServices::as_node(NodeServices::None),
                "::ffff:127.0.0.1",
                8333,
            ),
            NodeAddress(
                NodeServices::as_node(NodeServices::None),
                "::ffff:127.0.0.1",
                8333,
            ),
            MagicBytes::Mainnet,
            0,
            false,
        )
    }

    /// Connect to the node
    fn connect(&self) -> Result<TcpStream, NodeError> {
        println!("Connecting to node at {}", self.ip);
        TcpStream::connect(self.ip).map_err(NodeError::ConnectionError)
    }

    /// Convert IP address to bytes for a given IP address
    /// in the format of ipv4 or ipv6
    fn ip_as_bytes(ip: &'a str) -> Result<Vec<u8>, NodeError> {
        let ip_addr: IpAddr = ip.parse().map_err(|_| {
            NodeError::PayloadError(Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid IP address",
            ))
        })?;
        match ip_addr {
            IpAddr::V4(ipv4) => Ok(ipv4.octets().to_vec()),
            IpAddr::V6(ipv6) => Ok(ipv6.octets().to_vec()),
        }
    }

    /// Payload format
    /// https://developer.bitcoin.org/reference/p2p_networking.html#version
    ///
    /// version int32_t Required
    /// service uint64_t Required
    /// timestamp int64_t Required
    /// addr_recv services uint64_t
    /// addr_recv ip_address char[16]
    /// addr_recv port uint16_t
    /// addr_trans services uint64_t
    /// addr_trans ip_addr char[16]
    /// addr_trans port uint16_t
    /// nonce uint64_t
    /// user_agent_length compact_size
    /// user_agent char[]
    /// start_height int32_t
    /// relay uint8_t
    fn build_payload(&self, services: NodeServices) -> Result<Vec<u8>, NodeError> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs() as i64;

        let mut rng = rand::thread_rng();
        let nonce = rng.gen::<u64>();

        // version int32_t Required
        let mut payload = Vec::new();
        payload
            .write_i32::<LittleEndian>(self.version)
            .map_err(NodeError::PayloadError)?;

        // service uint64_t Required
        payload
            .write_u64::<LittleEndian>(NodeServices::as_node(services))
            .map_err(NodeError::PayloadError)?;

        // timestamp int64_t Required
        payload
            .write_i64::<LittleEndian>(timestamp)
            .map_err(NodeError::PayloadError)?;

        // write addr_recv and addr_trans
        for addr in [&self.addr_recv, &self.addr_trans] {
            // services
            payload
                .write_u64::<LittleEndian>(addr.0)
                .map_err(NodeError::PayloadError)?;

            // ip_address (must be 16 bytes)
            let mut ip_bytes = [0u8; 16];
            let raw_ip = Node::ip_as_bytes(addr.1)?;
            ip_bytes[16 - raw_ip.len()..].copy_from_slice(&raw_ip);
            payload.extend_from_slice(&ip_bytes);

            // port
            payload
                .write_u16::<BigEndian>(addr.2)
                .map_err(NodeError::PayloadError)?;
        }

        // nonce uint64_t
        payload
            .write_u64::<LittleEndian>(nonce)
            .map_err(NodeError::PayloadError)?;

        // user_agent
        // user_agent_length compact_size
        // user_agent char[]
        let user_agent = "/RustSeminar:0.0.1/rust:(1.87.0-nightly-3f5502370)/";
        let user_agent_length = compact_size(user_agent.len() as u64);
        payload.extend_from_slice(&user_agent_length);
        payload.extend_from_slice(user_agent.as_bytes());

        // start_height int32_t
        payload
            .write_i32::<LittleEndian>(self.start_height)
            .map_err(NodeError::PayloadError)?;

        // relay uint8_t
        payload
            .write_u8(self.relay as u8)
            .map_err(NodeError::PayloadError)?;

        Ok(payload)
    }

    /// Message header format
    /// https://developer.bitcoin.org/reference/p2p_networking.html#message-headers
    ///
    /// Start string: MagicBytes
    /// Command string: Command
    /// Payload: Vec<u8>
    /// Checksum: 4 bytes
    fn build_header(&self, command: Command, payload: &[u8]) -> Result<Vec<u8>, NodeError> {
        let mut message = Vec::new();

        message.extend_from_slice(self.magic_bytes.as_bytes());
        message.extend_from_slice(command.as_bytes());
        message
            .write_u32::<LittleEndian>(payload.len() as u32)
            .map_err(NodeError::WriteError)?;

        let checksum = self.checksum(payload)?;
        message.extend_from_slice(&checksum);

        Ok(message)
    }

    /// Double hash the payload using SHA256
    fn checksum(&self, payload: &[u8]) -> Result<[u8; 4], NodeError> {
        let hash = Sha256::digest(payload);
        let hash = Sha256::digest(hash);
        let mut checksum = [0u8; 4];
        checksum.copy_from_slice(&hash[0..4]);
        Ok(checksum)
    }

    /// Send version message to the node
    fn send_version(&self, stream: &mut TcpStream) -> Result<(), NodeError> {
        let payload = self.build_payload(NodeServices::None)?;
        let mut header = self.build_header(Command::Version, &payload)?;
        header.extend_from_slice(&payload);

        println!("Header: {:x?}", encode(&header));
        println!("Payload: {:x?}", encode(&payload));

        stream.write_all(&header).map_err(NodeError::SendError)?;
        Ok(())
    }

    /// Receive a Bitcoin message (with 24-byte header)
    fn receive_message(&self, stream: &mut TcpStream) -> Result<Vec<u8>, NodeError> {
        let mut header = [0u8; 24];
        stream
            .read_exact(&mut header)
            .map_err(NodeError::ReadError)?;

        let length = u32::from_le_bytes([header[16], header[17], header[18], header[19]]) as usize;

        let mut payload = vec![0u8; length];
        stream
            .read_exact(&mut payload)
            .map_err(NodeError::ReceiveError)?;

        Ok(payload)
    }
}

fn main() -> Result<(), NodeError> {
    let node = Node::create("<change-me-here>:8333");

    let mut stream = node.connect()?;

    node.send_version(&mut stream)?;

    let msg = node.receive_message(&mut stream)?;
    println!("Received message: {:x?}", encode(&msg));

    Ok(())
}

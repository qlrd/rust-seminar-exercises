use rusqlite;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SeminarNodeError {
    #[error("Malformed string: {0}")]
    MalformedString(std::net::AddrParseError),
    #[error("Connection error: {0}")]
    ConnectionError(std::io::Error),
    #[error("Encode error: {0}")]
    EncodeError(bitcoin::io::Error),
    #[error("Decode error: {0}")]
    DecodeError(bitcoin::consensus::encode::Error),
    #[error("Send error: {0}")]
    SendError(std::io::Error),
    #[error("Time error: {0}")]
    TimeError(std::time::SystemTimeError),
    #[error("PingPong error: {0}")]
    PingPongError(String),
    #[error("Max peers reached")]
    MaxPeersReached,
    #[error("Lock error: {0}")]
    PeerLockError(String),

    #[error("No peers available")]
    NoPeersAvailable,
    #[error("Create database error: {0}")]
    CreateDatabaseError(rusqlite::Error),
    #[error("Peer already exists: {0}:{1}")]
    PeerAlreadyExists(String, u16),
    #[error("Received zero bytes")]
    ZeroBytesRecvError,
    #[error("Max timeout reached")]
    TimeoutError,
    #[error("Connection closed")]
    ConnectionClosed,
    #[error("No peers found")]
    NoPeersFound,

    // Handshake errors
    #[error("Failed to handshake: {0}")]
    HandshakeError(String),

    // Database errors
    #[error("Open database error: {0}")]
    OpenDatabaseError(rusqlite::Error),

    #[error("Create peer error: {0}")]
    CreatePeerError(rusqlite::Error),

    #[error("Read peer error: {0}")]
    ReadPeerError(rusqlite::Error),

    #[error("Delete peer error: {0}")]
    DeletePeerError(rusqlite::Error),

    #[error("Update peer error: {0}")]
    UpdatePeerError(rusqlite::Error),

    #[error("Select peer error: {0}")]
    SelectPeerError(rusqlite::Error),

    #[error("Incomplete message")]
    IncompleteMessage,
}

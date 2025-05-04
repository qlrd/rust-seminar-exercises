use rusqlite;

#[derive(Debug)]
pub enum SeminarNodeError {
    MalformedString(std::net::AddrParseError),
    ConnectionError(std::io::Error),
    EncodeError(bitcoin::io::Error),
    DecodeError(bitcoin::consensus::encode::Error),
    SendError(std::io::Error),
    TimeError(std::time::SystemTimeError),
    HandshakeError(String),
    PingPongError(String),
    MaxPeersReached,
    PeerLockError(String),
    InsertPeerError(rusqlite::Error),
    SelectPeerError(rusqlite::Error),
    NoPeersAvailable,
    CreateDatabaseError(rusqlite::Error),
    PeerAlreadyExists(String, u16),
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
            SeminarNodeError::InsertPeerError(e) => write!(f, "Insert peer error: {e}"),
            SeminarNodeError::SelectPeerError(e) => write!(f, "Select peer error: {e}"),
            SeminarNodeError::CreateDatabaseError(err) => write!(f, "Create database error: {err}"),
            SeminarNodeError::PeerAlreadyExists(ip, port) => {
                write!(f, "Peer already exists: {ip}:{port}")
            }
        }
    }
}

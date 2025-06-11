use crate::error::SeminarNodeError;
use log::{debug, info, warn};
use rusqlite;
use std::fs;
use std::path::Path;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerStatus {
    Awaiting,
    Reachable,
    Active,
    Deactivated,
    Unreachable,
    Banned,
}

impl std::fmt::Display for PeerStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PeerStatus::Awaiting => write!(f, "awaiting"),
            PeerStatus::Reachable => write!(f, "reachable"),
            PeerStatus::Active => write!(f, "active"),
            PeerStatus::Deactivated => write!(f, "deactivated"),
            PeerStatus::Unreachable => write!(f, "unreachable"),
            PeerStatus::Banned => write!(f, "banned"),
        }
    }
}

impl FromStr for PeerStatus {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "awaiting" => Ok(PeerStatus::Awaiting),
            "active" => Ok(PeerStatus::Active),
            "deactivated" => Ok(PeerStatus::Deactivated),
            "unreachable" => Ok(PeerStatus::Unreachable),
            "banned" => Ok(PeerStatus::Banned),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Peer {
    pub id: u32,
    pub ip: String,
    pub port: u16,
    pub status: PeerStatus,
}

impl Peer {
    pub fn create_table<T: AsRef<Path>>(db_path: T) -> rusqlite::Result<(), rusqlite::Error> {
        debug!(
            "Creating peers table in database at: {}",
            db_path.as_ref().display()
        );
        let path = db_path.as_ref().join("peers.db");

        let _ = fs::create_dir_all(path.parent().unwrap());
        let conn = rusqlite::Connection::open(path)?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS peers (
                id INTEGER PRIMARY KEY,
                ip TEXT NOT NULL,
                port INTEGER NOT NULL,
                status TEXT NOT NULL
            )",
            [],
        )?;
        Ok(())
    }

    /// Create a new peer in the database
    ///
    /// This function will create a new peer in the database
    /// with the given IP address and port.
    pub fn create_peer<T: AsRef<Path>>(
        db_path: T,
        ip: String,
        port: u16,
    ) -> Result<u32, SeminarNodeError> {
        debug!("Creating peer {ip}:{port}");
        let path = db_path.as_ref().join("peers.db");
        let _ = fs::create_dir_all(path.parent().unwrap());

        let conn =
            rusqlite::Connection::open(path).map_err(SeminarNodeError::CreateDatabaseError)?;

        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM peers WHERE ip = ?1 AND port = ?2)",
                rusqlite::params![ip, port],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if exists {
            warn!("Peer {ip}:{port} already exists");
            return Err(SeminarNodeError::PeerAlreadyExists(ip, port));
        }

        conn.execute(
            "INSERT INTO peers (ip, port, status) VALUES (?1, ?2, ?3)",
            rusqlite::params![ip, port, PeerStatus::Awaiting.to_string()],
        )
        .map_err(SeminarNodeError::CreatePeerError)?;

        let mut stmt = conn
            .prepare("SELECT id, ip, port, status FROM peers WHERE ip = ?1 AND port = ?2")
            .map_err(SeminarNodeError::SelectPeerError)?;

        let peer = stmt
            .query_row([ip, format!("{port}")], |row| {
                Ok(Peer {
                    id: row.get(0)?,
                    ip: row.get(1)?,
                    port: row.get(2)?,
                    status: row.get(3).and_then(|status: String| {
                        PeerStatus::from_str(&status).map_err(|_| rusqlite::Error::InvalidQuery)
                    })?,
                })
            })
            .map_err(SeminarNodeError::SelectPeerError)?;

        info!("Created peer: {peer:?} ");
        Ok(peer.id)
    }

    /// Read a peer from the database
    ///
    /// This function will read a peer from the database
    /// with the given ID.
    pub fn read_peer<T: AsRef<Path>>(db_path: T, id: u32) -> Result<Peer, rusqlite::Error> {
        debug!("Reading peer with id: {id}");
        let path = db_path.as_ref().join("peers.db");
        let conn = rusqlite::Connection::open(path)?;
        let peer = conn.query_row(
            "SELECT id, ip, port, status FROM peers WHERE id = ?1",
            rusqlite::params![id],
            |row| {
                Ok(Peer {
                    id: row.get(0)?,
                    ip: row.get(1)?,
                    port: row.get(2)?,
                    status: row.get(3).and_then(|status: String| {
                        PeerStatus::from_str(&status).map_err(|_| rusqlite::Error::InvalidQuery)
                    })?,
                })
            },
        )?;

        info!("Read peer: {peer:?} ");
        Ok(peer)
    }

    /// Get a peer ID from the database
    ///
    /// This function will get a peer ID from the database
    /// with the given IP address and port.
    pub fn get_peer_id<T: AsRef<Path>>(
        db_path: T,
        ip: String,
        port: u16,
    ) -> Result<u32, SeminarNodeError> {
        debug!("Getting peer ID for {ip}:{port}");
        let path = db_path.as_ref().join("peers.db");
        let conn = rusqlite::Connection::open(path).map_err(SeminarNodeError::OpenDatabaseError)?;
        conn.query_row(
            "SELECT id FROM peers WHERE ip = ?1 AND port = ?2",
            rusqlite::params![ip, port],
            |row| row.get::<_, u32>(0),
        )
        .map_err(SeminarNodeError::ReadPeerError)
    }

    /// Update a peer status in the database
    ///
    /// Possible status are: [`PeerStatus::Active`], [`PeerStatus::Inactive`],
    /// [`PeerStatus::Awaiting`], [`PeerStatus::Banned`], [`PeerStatus::Unreachable`]
    pub fn update_peer<T: AsRef<Path>>(
        db_path: T,
        ip: String,
        port: u16,
        status: PeerStatus,
    ) -> Result<(), rusqlite::Error> {
        debug!("Updating peer {ip}:{port} to status {status}");
        let path = db_path.as_ref().join("peers.db");
        let conn = rusqlite::Connection::open(path)?;
        let peer = conn.execute(
            "UPDATE peers SET status = ?1 WHERE ip = ?2 AND port = ?3",
            rusqlite::params![status.to_string(), ip, port],
        )?;

        info!("Updated peer: id: {peer:?}, ip: {ip}, port: {port}, status: {status}");
        Ok(())
    }

    /// Delete a peer from the database
    ///
    /// Remove a peer from the database with the given ID.
    pub fn delete_peer<T: AsRef<Path>>(db_path: T, id: u32) -> Result<(), SeminarNodeError> {
        debug!("Deleting peer with id: {id}");
        let path = db_path.as_ref().join("peers.db");
        let conn = rusqlite::Connection::open(path).map_err(SeminarNodeError::OpenDatabaseError)?;
        conn.execute("DELETE FROM peers WHERE id = ?1", rusqlite::params![id])
            .map_err(SeminarNodeError::DeletePeerError);

        info!("Deleted peer with id: {id}");
        Ok(())
    }

    pub fn count<T: AsRef<Path>>(db_path: T) -> Result<u32, rusqlite::Error> {
        debug!(
            "Counting peers in database at: {}",
            db_path.as_ref().display()
        );
        let path = db_path.as_ref().join("peers.db");
        let conn = rusqlite::Connection::open(path)?;
        let count: u32 = conn.query_row("SELECT COUNT(*) FROM peers", [], |row| row.get(0))?;
        Ok(count)
    }

    pub fn get_all_peers<T: AsRef<Path>>(db_path: T) -> Result<Vec<Peer>, rusqlite::Error> {
        debug!(
            "Getting all peers from database at: {}",
            db_path.as_ref().display()
        );
        let path = db_path.as_ref().join("peers.db");
        let conn = rusqlite::Connection::open(path)?;
        let mut stmt = conn.prepare("SELECT id, ip, port, status FROM peers")?;

        let peer_iter = stmt.query_map([], |row| {
            Ok(Peer {
                id: row.get(0)?,
                ip: row.get(1)?,
                port: row.get(2)?,
                status: row.get::<_, String>(3).and_then(|s| {
                    PeerStatus::from_str(&s).map_err(|_| rusqlite::Error::InvalidQuery)
                })?,
            })
        })?;

        let mut peers = Vec::new();
        for peer in peer_iter {
            peers.push(peer?);
        }

        info!("Got {} peers from database", peers.len());
        Ok(peers)
    }
}

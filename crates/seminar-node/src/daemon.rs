use crate::error::SeminarNodeError;
use crate::node::SeminarNode;
use clap::{ArgAction, Parser};
use env_logger::Builder;
use log::LevelFilter;

#[derive(Parser, Debug)]
#[command(author = "qlrd")]
#[command(name = "seminard")]
#[command(about = "A command line interface for rust-seminar node daemon")]
pub struct Daemon {
    /// The ip address of the seminar node to connect to
    #[arg(short, long, value_name = "IP")]
    pub host: String,

    /// The port of the seminar node to connect to (default: 8333)
    #[arg(short, long, value_name = "PORT")]
    pub port: Option<u16>,

    /// Maximum number of connections (default: 8)
    #[arg(short, long, value_name = "MAX_PEERS")]
    pub max_peers: Option<u32>,

    /// Define if is a relay node (default is false)
    #[arg(short, long, action = ArgAction::SetFalse)]
    pub relay: Option<bool>,

    /// Log level
    #[arg(short, long, value_name = "LOG_LEVEL")]
    pub log_level: Option<String>,

    #[arg(short, long, value_name = "DB_PATH")]
    /// Path to the database file
    pub db_path: Option<String>,
}

impl Daemon {
    pub async fn run() -> Result<(), SeminarNodeError> {
        // Parse the command line arguments
        let args = Daemon::parse();
        let host = args.host;
        let port = args.port.unwrap_or(8333);
        let max_peers = args.max_peers.unwrap_or(8u32);
        let relay = args.relay.unwrap_or(false);
        let db_path = args.db_path.unwrap_or_else(|| {
            let home_dir = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            format!("{home_dir}/.seminar_node")
        });

        // Initialize the logger
        Builder::from_default_env()
            .format_timestamp_secs()
            .filter_level({
                if let Some(log_level) = args.log_level {
                    match log_level.as_str() {
                        "error" => LevelFilter::Error,
                        "warn" => LevelFilter::Warn,
                        "info" => LevelFilter::Info,
                        "debug" => LevelFilter::Debug,
                        "trace" => LevelFilter::Trace,
                        _ => LevelFilter::Info,
                    }
                } else {
                    LevelFilter::Info
                }
            })
            .init();

        // Create a SeminarNode instance connecting to an initial node
        let mut node = SeminarNode::create(host, port, max_peers, relay, db_path)?;
        node.run().await
    }
}

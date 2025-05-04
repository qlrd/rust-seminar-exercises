use seminar_node::error::SeminarNodeError;
use seminar_node::node::SeminarNode;

/// Write a Bitcoin client that connects to a public node using TCP.
///
/// 1. The IP address of the public node can be hardcoded for now
/// (we are changing that later). You can gather one using dig seed.bitcoin.sipa.be.
///
/// 2. Your client should perform the correct Bitcoin P2P protocol handshake.
///
/// 3.It is common for highly connected nodes to disconnect right after the handshake. You can check it by peeking the TcpStream.
///
///  * Your client should respond to ping messages otherwise the remote node will disconnect.
///  * To start receiving addresses of other nodes, send a getaddr message. You should receive addr messages from time to time.
///  * Print sent and received messages to the terminal using println!() (we are upgrading this later). Ignore received inv messages.`
fn main() -> Result<(), SeminarNodeError> {
    let ip = "<change-me>";

    // Initialize the logger
    env_logger::Builder::from_default_env()
        .format_timestamp_secs()
        .init();

    let home_dir = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let path = format!("{home_dir}/.seminar_node");

    // Create a SeminarNode instance
    // connecting to an initial node
    let mut node = SeminarNode::create(ip.to_string(), 8333, 8u32, false, path)?;

    node.run()
}

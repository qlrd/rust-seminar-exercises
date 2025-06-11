use seminar_node::daemon::Daemon;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize the seminar node daemon
    Daemon::run().await.unwrap_or_else(|err| {
        eprintln!("Error: {err}");
        std::process::exit(1);
    });
    Ok(())
}

use seminard::SeminarNodeDaemon;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize the seminar node daemon
    SeminarNodeDaemon::run().unwrap_or_else(|err| {
        eprintln!("Error: {err}");
        std::process::exit(1);
    });
    Ok(())
}

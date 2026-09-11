mod config;
mod error;
mod storage;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    tracing::info!("Binbot starting...");
    Ok(())
}

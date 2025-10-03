use journey_bot::config;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().init();
    tracing::info!("Starting up...");

    let config = config::config();
    journey_bot::launch(config).await.unwrap();
}

use std::path::PathBuf;

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();
    postamon_interno::install_crypto();
    let path = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("postamon-interno.toml"));
    let cfg = postamon_interno::config::load(&path)?;
    std::fs::create_dir_all(&cfg.data_dir)?;
    let listener = std::net::TcpListener::bind(cfg.bind)?;
    tracing::info!("Postamon interno {} in ascolto su {}", env!("CARGO_PKG_VERSION"), cfg.bind);
    postamon_interno::serve(cfg, listener).await
}

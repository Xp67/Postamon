//! Postamon interno: chat e allegati fra i PC dell'azienda, che non passano da
//! internet. Un solo programma, con i dati in una cartella (`interno.sqlite` e
//! `file/`).
//!
//! - `config`: `postamon-interno.toml`.
//! - `tls`: certificato autofirmato, generato alla prima partenza.
//! - `auth`: accesso con gli account del dominio (LDAPS) o utenti di prova.
//! - `db`: archivio SQLite.
//! - `hub`: connessioni aperte, per consegnare subito i messaggi.
//! - `api`: API HTTP e WebSocket.
//! - `scarica`: pagina di download di Postamon (Windows e Android).

pub mod api;
mod auth;
pub mod config;
mod db;
mod hub;
mod scarica;
pub mod tls;

use anyhow::Result;
use axum_server::tls_rustls::RustlsConfig;

pub use config::Config;

/// Da chiamare una volta all'avvio: la crittografia è quella di `ring`.
pub fn install_crypto() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

pub async fn serve(cfg: Config, listener: std::net::TcpListener) -> Result<()> {
    let state = api::AppState::new(cfg.clone())?;
    let app = api::router(state);
    // Tokio vuole il socket non bloccante (su Windows, se no, le connessioni
    // cifrate restano appese).
    listener.set_nonblocking(true)?;
    if cfg.tls {
        let cert = tls::load_or_create(&cfg.data_dir)?;
        tracing::info!("impronta del certificato: {}", cert.fingerprint);
        let rustls = RustlsConfig::from_pem(cert.cert_pem, cert.key_pem).await?;
        axum_server::tls_rustls::from_tcp_rustls(listener, rustls)?.serve(app.into_make_service()).await?;
    } else {
        axum::serve(tokio::net::TcpListener::from_std(listener)?, app).await?;
    }
    Ok(())
}

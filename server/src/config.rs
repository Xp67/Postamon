//! Configurazione del server (`postamon-interno.toml`).

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    #[serde(default = "default_bind")]
    pub bind: SocketAddr,
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
    /// Dimensione massima di un allegato.
    #[serde(default = "default_max_file_mb")]
    pub max_file_mb: u64,
    /// Connessione cifrata con il certificato generato dal server. Si spegne
    /// solo nei test automatici.
    #[serde(default = "default_true")]
    pub tls: bool,
    /// Accesso con indirizzo email e password della casella di posta.
    pub imap: Option<Imap>,
    /// Accesso con gli account del dominio.
    pub ldap: Option<Ldap>,
    /// Utenti di prova, in aggiunta a quelli del dominio.
    #[serde(default)]
    pub local_users: Vec<LocalUser>,
}

/// Server di posta su cui si verificano email e password (IMAP con TLS).
#[derive(Clone, Debug, Deserialize)]
pub struct Imap {
    /// `in.postassl.it`
    pub host: String,
    #[serde(default = "default_imap_port")]
    pub port: u16,
    /// Domini ammessi (`dimensionesalotto.it`); vuoto = qualunque.
    #[serde(default)]
    pub allowed_domains: Vec<String>,
}

fn default_imap_port() -> u16 {
    993
}

#[derive(Clone, Debug, Deserialize)]
pub struct Ldap {
    /// `ldaps://controller.azienda.local:636`
    pub url: String,
    /// Suffisso degli account: `azienda.local` per `mario@azienda.local`.
    pub upn_suffix: String,
    /// `DC=AZIENDA,DC=local`
    pub base_dn: String,
    /// Il certificato del controller di dominio di solito non è di
    /// un'autorità pubblica: la verifica è spenta salvo indicazione.
    #[serde(default)]
    pub verify_tls: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LocalUser {
    pub username: String,
    pub display_name: String,
    pub password: String,
}

fn default_bind() -> SocketAddr {
    "0.0.0.0:8790".parse().expect("indirizzo predefinito")
}

fn default_data_dir() -> PathBuf {
    PathBuf::from("dati")
}

fn default_max_file_mb() -> u64 {
    4096
}

fn default_true() -> bool {
    true
}

pub fn load(path: &Path) -> Result<Config> {
    let text = std::fs::read_to_string(path).with_context(|| format!("lettura di {}", path.display()))?;
    let mut cfg: Config = toml::from_str(&text).with_context(|| format!("configurazione {}", path.display()))?;
    if cfg.data_dir.is_relative()
        && let Some(dir) = path.parent()
    {
        cfg.data_dir = dir.join(&cfg.data_dir);
    }
    Ok(cfg)
}

impl Config {
    pub fn max_file_bytes(&self) -> u64 {
        self.max_file_mb * 1024 * 1024
    }
}

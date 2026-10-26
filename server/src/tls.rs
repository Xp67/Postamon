//! Certificato del server. Alla prima partenza se ne genera uno autofirmato;
//! Postamon ne memorizza l'impronta al primo collegamento e da lì in poi
//! accetta solo quel certificato.

use std::path::Path;

use anyhow::{Context, Result, anyhow};
use base64::Engine;
use sha2::{Digest, Sha256};

pub struct ServerCert {
    pub cert_pem: Vec<u8>,
    pub key_pem: Vec<u8>,
    pub fingerprint: String,
}

pub fn load_or_create(dir: &Path) -> Result<ServerCert> {
    let cert_path = dir.join("certificato.pem");
    let key_path = dir.join("chiave.pem");
    if !cert_path.is_file() || !key_path.is_file() {
        let generated = rcgen::generate_simple_self_signed(vec!["postamon-interno".to_string(), "localhost".to_string()])
            .context("generazione del certificato")?;
        std::fs::write(&cert_path, generated.cert.pem())?;
        std::fs::write(&key_path, generated.signing_key.serialize_pem())?;
        restrict(&key_path)?;
    }
    let cert_pem = std::fs::read(&cert_path)?;
    let key_pem = std::fs::read(&key_path)?;
    let fingerprint = fingerprint(&pem_to_der(&cert_pem)?);
    Ok(ServerCert { cert_pem, key_pem, fingerprint })
}

/// SHA-256 del certificato (DER), in esadecimale.
pub fn fingerprint(der: &[u8]) -> String {
    Sha256::digest(der).iter().map(|b| format!("{b:02x}")).collect()
}

fn pem_to_der(pem: &[u8]) -> Result<Vec<u8>> {
    let text = std::str::from_utf8(pem)?;
    let body: String = text
        .lines()
        .skip_while(|l| !l.starts_with("-----BEGIN CERTIFICATE-----"))
        .skip(1)
        .take_while(|l| !l.starts_with("-----END CERTIFICATE-----"))
        .collect();
    if body.is_empty() {
        return Err(anyhow!("certificato.pem senza certificato"));
    }
    Ok(base64::engine::general_purpose::STANDARD.decode(body)?)
}

#[cfg(unix)]
fn restrict(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_once_and_keeps_the_same_fingerprint() {
        let dir = std::env::temp_dir().join(format!("postamon-interno-tls-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let first = load_or_create(&dir).unwrap();
        let second = load_or_create(&dir).unwrap();
        assert_eq!(first.fingerprint, second.fingerprint);
        assert_eq!(first.fingerprint.len(), 64);
        let _ = std::fs::remove_dir_all(dir);
    }
}

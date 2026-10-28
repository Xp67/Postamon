//! Caselle configurate. I dati stanno in `accounts.json`, le password nella
//! Gestione credenziali di Windows: su disco non finiscono mai in chiaro.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

/// Le copie di sviluppo hanno password proprie: togliere una casella da
/// Postamon Dev non deve cancellare quella del Postamon installato.
const KEYRING_SERVICE: &str = if cfg!(debug_assertions) { "Postamon Dev" } else { "Postamon" };

/// Il Postamon installato: identificativo (la sua cartella dati) e nome delle
/// sue password. Postamon Dev ne prende le caselle, vedi `import_installed`.
const INSTALLED_IDENTIFIER: &str = "it.dimensionesalotto.postamon";
const INSTALLED_SERVICE: &str = "Postamon";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub email: String,
    /// Nome mostrato ai destinatari ("Marco").
    pub name: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    /// Firma dei messaggi nuovi e firma di risposte e inoltri: id
    /// nell'elenco delle firme (`signatures.json`), nessuna se `None`.
    #[serde(default)]
    pub signature_new: Option<String>,
    #[serde(default)]
    pub signature_reply: Option<String>,
    /// Firma di testo delle versioni precedenti. Alla prima partenza con
    /// l'elenco delle firme ne diventa una voce (`signatures::first_run`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub signature: String,
    /// Solo per le prove con un server locale a certificato autofirmato.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub accept_invalid_certs: bool,
}

pub struct Accounts {
    path: PathBuf,
    list: Mutex<Vec<Account>>,
}

impl Accounts {
    pub fn load(path: PathBuf) -> Result<Self> {
        let list = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).context("lettura di accounts.json")?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e).context("lettura di accounts.json"),
        };
        Ok(Self { path, list: Mutex::new(list) })
    }

    pub fn all(&self) -> Vec<Account> {
        self.lock().clone()
    }

    pub fn get(&self, id: &str) -> Result<Account> {
        self.lock()
            .iter()
            .find(|a| a.id == id)
            .cloned()
            .ok_or_else(|| anyhow!("Casella non trovata: {id}"))
    }

    pub fn contains(&self, id: &str) -> bool {
        self.lock().iter().any(|a| a.id == id)
    }

    pub fn upsert(&self, account: Account) -> Result<()> {
        let mut list = self.lock();
        match list.iter_mut().find(|a| a.id == account.id) {
            Some(slot) => *slot = account,
            None => list.push(account),
        }
        save(&self.path, &list)
    }

    pub fn remove(&self, id: &str) -> Result<()> {
        let mut list = self.lock();
        list.retain(|a| a.id != id);
        save(&self.path, &list)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Account>> {
        self.list.lock().unwrap_or_else(|p| p.into_inner())
    }
}

fn save(path: &Path, list: &[Account]) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(list)?).context("salvataggio delle caselle")?;
    std::fs::rename(&tmp, path).context("salvataggio delle caselle")?;
    Ok(())
}

pub fn account_id(email: &str) -> String {
    email.trim().to_lowercase()
}

/// Postamon Dev, la prima volta, parte con le caselle del Postamon installato:
/// solo l'elenco (con le firme), posta e chat le scarica da sé. Le password
/// restano quelle dell'installato, vedi `password`.
pub fn import_installed(dir: &Path) {
    let file = dir.join("accounts.json");
    let Some(source) = dir.parent().map(|p| p.join(INSTALLED_IDENTIFIER).join("accounts.json")) else {
        return;
    };
    if file.exists() || source == file || !source.exists() {
        return;
    }
    if let Err(e) = std::fs::copy(&source, &file) {
        eprintln!("caselle del Postamon installato non copiate: {e}");
    }
}

pub fn password(account: &Account) -> Result<String> {
    let mut found = keyring::Entry::new(KEYRING_SERVICE, &account.email)?.get_password();
    // Postamon Dev, finché non ha una password sua per la casella, usa quella
    // del Postamon installato: la legge soltanto, non la cambia né la cancella.
    if cfg!(debug_assertions) && matches!(found, Err(keyring::Error::NoEntry)) {
        found = keyring::Entry::new(INSTALLED_SERVICE, &account.email)?.get_password();
    }
    found.map_err(|e| match e {
            keyring::Error::NoEntry => anyhow!(
                "La password di {} non è salvata: togli la casella e aggiungila di nuovo",
                account.email
            ),
            e => anyhow!("Lettura della password non riuscita: {e}"),
        })
}

pub fn set_password(email: &str, password: &str) -> Result<()> {
    keyring::Entry::new(KEYRING_SERVICE, email)?
        .set_password(password)
        .context("salvataggio della password nella Gestione credenziali")
}

pub fn delete_password(email: &str) {
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, email) {
        let _ = entry.delete_credential();
    }
}

/// Firma di Outlook (versione testo) della casella, se Outlook ne ha una
/// con l'indirizzo nel nome, come "Marco (marco@dominio.it).txt". Serve solo
/// a riconoscere le firme di testo copiate da lì dalle versioni precedenti.
pub fn outlook_signature(email: &str) -> Option<String> {
    let dir = PathBuf::from(std::env::var_os("APPDATA")?).join("Microsoft").join("Signatures");
    let needle = email.to_lowercase();
    let file = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| {
        p.extension().is_some_and(|x| x.eq_ignore_ascii_case("txt"))
            && p.file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.to_lowercase().contains(&needle))
    })?;
    let text = decode_text(&std::fs::read(file).ok()?);
    let text = text.replace("\r\n", "\n").trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// Outlook salva le firme .txt in UTF-16 o nella codifica di Windows.
fn decode_text(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        return String::from_utf16_lossy(&units);
    }
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        // windows-1252 coincide con Latin-1 per le lettere accentate italiane.
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf16_and_latin1_signatures() {
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("Città".encode_utf16().flat_map(|u| u.to_le_bytes()))
            .collect();
        assert_eq!(decode_text(&utf16), "Città");
        assert_eq!(decode_text(&[b'C', b'i', b't', b't', 0xE0]), "Città");
        assert_eq!(decode_text("Città".as_bytes()), "Città");
    }

    #[test]
    fn dev_starts_with_installed_accounts() {
        let root = std::env::temp_dir().join(format!("postamon-import-{}", std::process::id()));
        let installed = root.join(INSTALLED_IDENTIFIER);
        let dev = root.join(format!("{INSTALLED_IDENTIFIER}.dev"));
        std::fs::create_dir_all(&installed).unwrap();
        std::fs::create_dir_all(&dev).unwrap();
        std::fs::write(installed.join("accounts.json"), "[1]").unwrap();

        import_installed(&dev);
        assert_eq!(std::fs::read_to_string(dev.join("accounts.json")).unwrap(), "[1]");
        // Dopo la prima volta le caselle di Postamon Dev restano le sue
        std::fs::write(dev.join("accounts.json"), "[2]").unwrap();
        import_installed(&dev);
        assert_eq!(std::fs::read_to_string(dev.join("accounts.json")).unwrap(), "[2]");
        // Sulla cartella dell'installato non fa niente
        import_installed(&installed);
        assert_eq!(std::fs::read_to_string(installed.join("accounts.json")).unwrap(), "[1]");
        let _ = std::fs::remove_dir_all(root);
    }
}

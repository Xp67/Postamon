//! Add-on: piccole app web che aggiungono funzioni a Postamon. La guida per
//! chi li scrive è ADDONS.md nella radice del repo.
//!
//! **Dove stanno.** Il sorgente di ogni add-on è nel repo, in `addons/<id>/`,
//! e non entra nella build di Postamon. Dopo il merge della pull request,
//! `scripts/pubblica-addon.ps1` li impacchetta e pubblica il catalogo firmato
//! (`addons.json` + `addons.json.sig`, chiave pubblica in [`PUBKEY`]).
//! Postamon scarica solo gli add-on che l'utente installa dal marketplace,
//! controlla la firma del catalogo e lo SHA-256 del pacchetto e li tiene in
//! `<dati>/addons/<id>/<versione>/`; i dati dell'add-on stanno in
//! `<dati>/addons/<id>/data.json`. Disinstallare cancella tutto.
//!
//! **Isolamento.** Le pagine si servono da un server su 127.0.0.1 e
//! l'interfaccia le mostra in un iframe `sandbox="allow-scripts allow-forms"`:
//! origine opaca, quindi niente accesso all'interfaccia, ai comandi di
//! Postamon o al localStorage. Ogni risposta porta una Content-Security-Policy
//! che vieta qualunque collegamento fuori dal pacchetto, salvo i domini dei
//! permessi `network:`; la pagina principale vieta ai riquadri di navigare
//! fuori da 127.0.0.1 (`frame-src` in index.html). L'add-on parla con
//! Postamon solo con postMessage (sdk/postamon-addon.js) e Postamon risponde
//! solo a ciò che i permessi concedono. Lo script di Tauri arriva anche nei
//! riquadri (WebView2 lo inietta ovunque), ma le chiamate da un'origine opaca
//! portano `Origin: null` e Tauri le scarta: per questo il riquadro non deve
//! mai avere `allow-same-origin`.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Versione dell'API per gli add-on (campo `api` di addon.json).
pub const API_VERSION: u32 = 1;

/// Il catalogo sta nella release `addons` del repo di Postamon.
const CATALOG_URL: &str = "https://github.com/Xp67/Postamon/releases/download/addons/";

/// Chiave pubblica che firma il catalogo. La privata è
/// `~\.tauri\postamon-addons.key` sul PC di chi pubblica, mai nel repo.
const PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDUyQzBBMUMwREU0RTRBMTQKUldRVVNrN2V3S0hBVXFyODd2MWp3N3FCNmZSMkdYNmlpemErdXFzbjV3NURTRnJXN1dtSDN0YlQK";

/// Permessi oltre a `network:<dominio>`.
pub const PERMISSIONS: &[&str] = &["message.read", "compose"];
/// Punti di aggancio dell'interfaccia.
pub const HOOKS: &[&str] = &["message-button", "sidebar-view"];

/// Tipi di file ammessi in un pacchetto.
const FILE_TYPES: &[&str] = &[
    "html", "js", "mjs", "css", "json", "svg", "png", "jpg", "jpeg", "gif", "webp", "ico", "woff", "woff2", "ttf",
    "otf", "txt", "md",
];
const MAX_PACKAGE: u64 = 5 * 1024 * 1024;
const MAX_UNPACKED: u64 = 20 * 1024 * 1024;
const MAX_FILES: usize = 300;
const MAX_CATALOG: u64 = 2 * 1024 * 1024;
const MAX_STORAGE: usize = 1024 * 1024;
const MAX_HOSTS: usize = 5;

const SDK_JS: &str = include_str!("../../sdk/postamon-addon.js");
const SDK_CSS: &str = include_str!("../../sdk/postamon-addon.css");

// ---- addon.json ----

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub api: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_postamon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    pub hooks: Vec<Hook>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hook {
    #[serde(rename = "type")]
    pub kind: String,
    pub title: String,
    pub page: String,
}

impl Manifest {
    pub fn validate(&self) -> Result<()> {
        if !valid_id(&self.id) {
            bail!("id «{}» non valido: da 2 a 40 caratteri fra a-z, 0-9 e trattino", self.id);
        }
        check_text("name", &self.name, 60)?;
        check_text("description", &self.description, 300)?;
        check_text("author", &self.author, 80)?;
        semver::Version::parse(&self.version)
            .map_err(|_| anyhow!("version «{}» non è nel formato 1.2.3", self.version))?;
        if self.api == 0 {
            bail!("api deve essere almeno 1");
        }
        if let Some(v) = &self.min_postamon {
            semver::Version::parse(v).map_err(|_| anyhow!("minPostamon «{v}» non è nel formato 1.2.3"))?;
        }
        if let Some(icon) = &self.icon {
            package_path(icon)?;
            if !(icon.ends_with(".svg") || icon.ends_with(".png")) {
                bail!("icon deve essere un file .svg o .png");
            }
        }
        let mut hosts = 0;
        for (i, p) in self.permissions.iter().enumerate() {
            if let Some(host) = p.strip_prefix("network:") {
                if !valid_host(host) {
                    bail!("permesso «{p}»: serve un nome di dominio come api.esempio.it");
                }
                hosts += 1;
            } else if !PERMISSIONS.contains(&p.as_str()) {
                bail!("permesso sconosciuto: «{p}»");
            }
            if self.permissions[..i].contains(p) {
                bail!("permesso ripetuto: «{p}»");
            }
        }
        if hosts > MAX_HOSTS {
            bail!("al massimo {MAX_HOSTS} domini nei permessi network:");
        }
        if self.hooks.is_empty() || self.hooks.len() > 8 {
            bail!("hooks deve avere da 1 a 8 punti di aggancio");
        }
        for h in &self.hooks {
            if !HOOKS.contains(&h.kind.as_str()) {
                bail!("punto di aggancio sconosciuto: «{}»", h.kind);
            }
            check_text("title", &h.title, 40)?;
            package_path(&h.page)?;
            if !h.page.ends_with(".html") {
                bail!("la pagina di un punto di aggancio deve essere un file .html");
            }
        }
        Ok(())
    }

    /// Perché questa versione di Postamon non può usare l'add-on.
    pub fn incompatible(&self) -> Option<String> {
        if self.api > API_VERSION {
            return Some(format!("Richiede l'API {} degli add-on: aggiorna Postamon", self.api));
        }
        let min = semver::Version::parse(self.min_postamon.as_deref()?).ok()?;
        let current = semver::Version::parse(env!("CARGO_PKG_VERSION")).ok()?;
        (current < min).then(|| format!("Richiede Postamon {min} o più nuovo"))
    }
}

fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    (2..=40).contains(&b.len())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
        && b[0] != b'-'
        && b[b.len() - 1] != b'-'
}

/// Nome di dominio come `api.esempio.it` (niente IP, porte o asterischi).
fn valid_host(host: &str) -> bool {
    let labels: Vec<&str> = host.split('.').collect();
    host.len() <= 253
        && labels.len() >= 2
        && labels.iter().all(|l| {
            (1..=63).contains(&l.len())
                && l.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
                && !l.starts_with('-')
                && !l.ends_with('-')
        })
        && !labels[labels.len() - 1].bytes().all(|c| c.is_ascii_digit())
}

fn check_text(field: &str, value: &str, max: usize) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{field} è vuoto");
    }
    if value.chars().count() > max {
        bail!("{field} supera i {max} caratteri");
    }
    if value.chars().any(char::is_control) {
        bail!("{field} contiene caratteri di controllo");
    }
    Ok(())
}

/// Percorso di un file dentro il pacchetto: relativo, senza `..`, con
/// lettere, cifre, `.`, `_`, `-` e `/`, di un tipo ammesso, fuori da
/// `_postamon/` (riservata alla libreria servita da Postamon).
fn package_path(path: &str) -> Result<()> {
    let ok_chars = path.bytes().all(|c| c.is_ascii_alphanumeric() || b"._-/".contains(&c));
    let ok_parts = !path.is_empty()
        && path.len() <= 200
        && !path.starts_with('/')
        && Path::new(path).components().all(|c| matches!(c, Component::Normal(_)))
        && !path.split('/').any(|p| p.is_empty() || p.starts_with('.'));
    if !ok_chars || !ok_parts {
        bail!("percorso non ammesso: «{path}»");
    }
    if path == "_postamon" || path.starts_with("_postamon/") {
        bail!("_postamon/ è riservata a Postamon: «{path}»");
    }
    let ext = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if !FILE_TYPES.contains(&ext.as_str()) {
        bail!("tipo di file non ammesso: «{path}»");
    }
    Ok(())
}

fn read_manifest(dir: &Path) -> Result<Manifest> {
    let bytes = std::fs::read(dir.join("addon.json")).with_context(|| format!("manca addon.json in {}", dir.display()))?;
    let text = std::str::from_utf8(&bytes).context("addon.json non è testo UTF-8")?;
    let m: Manifest = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| anyhow!("addon.json: {e}"))?;
    m.validate()?;
    for page in m.hooks.iter().map(|h| &h.page).chain(m.icon.iter()) {
        if !dir.join(page).is_file() {
            bail!("addon.json indica {page}, che non c'è");
        }
    }
    Ok(m)
}

/// Content-Security-Policy delle pagine di un add-on: solo i suoi file, più
/// i domini dei permessi `network:` per i collegamenti e le immagini.
fn csp(origin: &str, id: &str, granted: &[String]) -> String {
    let own = format!("{origin}/a/{id}/");
    let net: String = granted
        .iter()
        .filter_map(|p| p.strip_prefix("network:"))
        .map(|h| format!(" https://{h}"))
        .collect();
    format!(
        "default-src 'none'; script-src {own}; style-src {own} 'unsafe-inline'; img-src {own} data: blob:{net}; \
         font-src {own} data:; media-src {own} data: blob:; connect-src {own}{net}; worker-src {own}; \
         form-action 'none'; frame-src 'none'; base-uri 'none'; object-src 'none'"
    )
}

// ---- catalogo ----

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    #[serde(flatten)]
    pub manifest: Manifest,
    /// Nome del pacchetto accanto al catalogo (`<id>-<versione>.zip`).
    pub file: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Deserialize)]
struct CatalogFile {
    addons: Vec<CatalogEntry>,
}

/// Voce del marketplace, con lo stato dell'add-on su questo PC.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogItem {
    #[serde(flatten)]
    pub entry: CatalogEntry,
    pub installed_version: Option<String>,
    /// Caricato in modalità sviluppatore con lo stesso id.
    pub dev: bool,
    pub update: bool,
    pub incompatible: Option<String>,
    /// Permessi che l'utente non ha ancora concesso.
    pub new_permissions: Vec<String>,
}

fn verify_signature(pubkey_b64: &str, data: &[u8], sig_b64: &str) -> Result<()> {
    let b64 = base64::engine::general_purpose::STANDARD;
    let pk_text = String::from_utf8(b64.decode(pubkey_b64.trim())?)?;
    let pk = minisign_verify::PublicKey::decode(&pk_text).map_err(|e| anyhow!("chiave del catalogo: {e}"))?;
    let invalid = || anyhow!("Il catalogo degli add-on non ha una firma valida: non lo uso");
    let sig_text = String::from_utf8(b64.decode(sig_b64.trim()).map_err(|_| invalid())?).map_err(|_| invalid())?;
    let sig = minisign_verify::Signature::decode(&sig_text).map_err(|_| invalid())?;
    pk.verify(data, &sig, false).map_err(|_| invalid())
}

fn package_name_ok(name: &str) -> bool {
    name.ends_with(".zip") && name.len() <= 100 && name.bytes().all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
}

fn newer(a: &str, b: &str) -> bool {
    match (semver::Version::parse(a), semver::Version::parse(b)) {
        (Ok(a), Ok(b)) => a > b,
        _ => false,
    }
}

fn same_set(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && a.iter().all(|x| b.contains(x))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Estrae un pacchetto controllando numero, tipo, percorso e dimensione dei file.
fn extract_package(data: &[u8], dest: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(data)).context("pacchetto illeggibile")?;
    if archive.len() > MAX_FILES {
        bail!("pacchetto con troppi file");
    }
    let mut total = 0u64;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().map_err(|_| anyhow!("nome di file illeggibile nel pacchetto"))?.to_string();
        package_path(&name)?;
        let out = dest.join(&name);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::File::create(&out)?;
        // Il limite vale sui byte veri, non sulla dimensione dichiarata nello zip.
        let written = std::io::copy(&mut (&mut entry).take(MAX_UNPACKED - total + 1), &mut file)?;
        total += written;
        if total > MAX_UNPACKED {
            bail!("pacchetto troppo grande una volta estratto");
        }
    }
    Ok(())
}

// ---- add-on installati ----

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    version: String,
    enabled: bool,
    /// Permessi concessi dall'utente all'installazione.
    granted: Vec<String>,
    /// Cartella del sorgente, per gli add-on caricati in modalità sviluppatore.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dev_path: Option<PathBuf>,
}

/// Add-on installato, come lo vede l'interfaccia.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledAddon {
    pub id: String,
    pub version: String,
    pub enabled: bool,
    pub dev: bool,
    pub granted: Vec<String>,
    /// Assente se addon.json non si legge (`error` dice perché).
    pub manifest: Option<Manifest>,
    /// Indirizzo della cartella del pacchetto, con la barra finale.
    pub base_url: String,
    pub dev_path: Option<String>,
    pub error: Option<String>,
}

pub enum StorageOp {
    Get(String),
    Set(String, Value),
    Remove(String),
    Keys,
}

#[derive(Clone)]
struct Site {
    dir: PathBuf,
    mount: String,
    csp: String,
    dev: bool,
}

struct Shared {
    port: u16,
    sites: Mutex<BTreeMap<String, Site>>,
}

pub struct Addons {
    root: PathBuf,
    shared: Arc<Shared>,
    records: Mutex<BTreeMap<String, Record>>,
    http: reqwest::Client,
    catalog_url: String,
    pubkey: String,
    install_lock: tokio::sync::Mutex<()>,
    storage_lock: Mutex<()>,
}

impl Addons {
    /// `POSTAMON_ADDONS_URL` (cartella con addons.json, con la barra finale)
    /// serve per provare un catalogo diverso da quello pubblicato.
    pub fn start(root: PathBuf) -> Result<Self> {
        let url = std::env::var("POSTAMON_ADDONS_URL").unwrap_or_else(|_| CATALOG_URL.to_string());
        Self::with(root, url, PUBKEY.to_string())
    }

    fn with(root: PathBuf, catalog_url: String, pubkey: String) -> Result<Self> {
        std::fs::create_dir_all(&root)?;
        let server = tiny_http::Server::http("127.0.0.1:0").map_err(|e| anyhow!("server degli add-on: {e}"))?;
        let port = server
            .server_addr()
            .to_ip()
            .map(|a| a.port())
            .ok_or_else(|| anyhow!("server degli add-on senza porta"))?;
        let records: BTreeMap<String, Record> = std::fs::read(root.join("installed.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let shared = Arc::new(Shared { port, sites: Mutex::default() });
        let serving = shared.clone();
        std::thread::Builder::new().name("add-on".into()).spawn(move || {
            for request in server.incoming_requests() {
                let shared = serving.clone();
                std::thread::spawn(move || handle(&shared, request));
            }
        })?;
        let http = reqwest::Client::builder()
            .user_agent(concat!("Postamon/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(120))
            .build()?;
        let addons = Self {
            root,
            shared,
            records: Mutex::new(records),
            http,
            catalog_url,
            pubkey,
            install_lock: tokio::sync::Mutex::new(()),
            storage_lock: Mutex::new(()),
        };
        addons.installed();
        Ok(addons)
    }

    pub fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.shared.port)
    }

    fn records(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Record>> {
        self.records.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn save(&self, records: &BTreeMap<String, Record>) -> Result<()> {
        write_atomic(&self.root.join("installed.json"), &serde_json::to_vec_pretty(records)?)
    }

    /// Add-on installati. Rilegge addon.json (per quelli in sviluppo, così
    /// una modifica si vede con "Ricarica") e prepara le pagine da servire.
    pub fn installed(&self) -> Vec<InstalledAddon> {
        let origin = self.origin();
        let mut records = self.records();
        let mut sites = BTreeMap::new();
        let mut list = Vec::new();
        let mut changed = false;
        for (id, rec) in records.iter_mut() {
            let dir = match &rec.dev_path {
                Some(p) => p.clone(),
                None => self.root.join(id).join(&rec.version),
            };
            let (manifest, error) = match read_manifest(&dir) {
                Ok(m) if &m.id != id => (None, Some(format!("addon.json dice id «{}», ma è installato come «{id}»", m.id))),
                Ok(m) => (Some(m), None),
                Err(e) => (None, Some(format!("{e:#}"))),
            };
            if let (Some(m), Some(_)) = (&manifest, &rec.dev_path)
                && (rec.granted != m.permissions || rec.version != m.version)
            {
                rec.granted = m.permissions.clone();
                rec.version = m.version.clone();
                changed = true;
            }
            let mount = if rec.dev_path.is_some() { "dev".to_string() } else { rec.version.clone() };
            if manifest.is_some() && rec.enabled {
                sites.insert(
                    id.clone(),
                    Site {
                        dir: dir.clone(),
                        mount: mount.clone(),
                        csp: csp(&origin, id, &rec.granted),
                        dev: rec.dev_path.is_some(),
                    },
                );
            }
            list.push(InstalledAddon {
                id: id.clone(),
                version: rec.version.clone(),
                enabled: rec.enabled,
                dev: rec.dev_path.is_some(),
                granted: rec.granted.clone(),
                manifest,
                base_url: format!("{origin}/a/{id}/{mount}/"),
                dev_path: rec.dev_path.as_ref().map(|p| p.display().to_string()),
                error,
            });
        }
        if changed {
            let _ = self.save(&records);
        }
        *self.shared.sites.lock().unwrap_or_else(|p| p.into_inner()) = sites;
        list
    }

    // ---- catalogo ----

    async fn fetch(&self, name: &str, max: u64) -> Result<Vec<u8>> {
        let url = format!("{}{name}", self.catalog_url);
        let response = self.http.get(&url).send().await.context("Catalogo degli add-on non raggiungibile")?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            bail!("Il catalogo degli add-on non è ancora pubblicato");
        }
        let response = response.error_for_status().context("Catalogo degli add-on non disponibile")?;
        if response.content_length().is_some_and(|n| n > max) {
            bail!("{name} è troppo grande");
        }
        let mut data = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            data.extend_from_slice(&chunk.context("download interrotto")?);
            if data.len() as u64 > max {
                bail!("{name} è troppo grande");
            }
        }
        Ok(data)
    }

    /// Catalogo pubblicato, dopo il controllo della firma. Le voci non valide
    /// si scartano.
    async fn catalog(&self) -> Result<Vec<CatalogEntry>> {
        let json = self.fetch("addons.json", MAX_CATALOG).await?;
        let sig = self.fetch("addons.json.sig", 4096).await?;
        verify_signature(&self.pubkey, &json, &String::from_utf8_lossy(&sig))?;
        let file: CatalogFile = serde_json::from_slice(&json).context("Catalogo degli add-on illeggibile")?;
        Ok(file
            .addons
            .into_iter()
            .filter(|e| match e.manifest.validate() {
                Ok(()) if package_name_ok(&e.file) => true,
                Ok(()) => false,
                Err(err) => {
                    eprintln!("add-on {} scartato dal catalogo: {err:#}", e.manifest.id);
                    false
                }
            })
            .collect())
    }

    pub async fn catalog_items(&self) -> Result<Vec<CatalogItem>> {
        let entries = self.catalog().await?;
        let records = self.records().clone();
        Ok(entries
            .into_iter()
            .map(|entry| {
                let rec = records.get(&entry.manifest.id);
                let granted: &[String] = rec.map(|r| r.granted.as_slice()).unwrap_or(&[]);
                CatalogItem {
                    installed_version: rec.map(|r| r.version.clone()),
                    dev: rec.is_some_and(|r| r.dev_path.is_some()),
                    update: rec.is_some_and(|r| r.dev_path.is_none() && newer(&entry.manifest.version, &r.version)),
                    incompatible: entry.manifest.incompatible(),
                    new_permissions: entry.manifest.permissions.iter().filter(|p| !granted.contains(p)).cloned().collect(),
                    entry,
                }
            })
            .collect())
    }

    /// Installa o aggiorna un add-on del catalogo. `accepted` sono i permessi
    /// che l'utente ha visto e accettato: se nel frattempo il catalogo ne
    /// chiede altri, non si installa niente.
    pub async fn install(&self, id: &str, accepted: &[String]) -> Result<()> {
        let _guard = self.install_lock.lock().await;
        let entry = self
            .catalog()
            .await?
            .into_iter()
            .find(|e| e.manifest.id == id)
            .ok_or_else(|| anyhow!("{id} non è nel catalogo"))?;
        let m = &entry.manifest;
        if let Some(why) = m.incompatible() {
            bail!("{}: {why}", m.name);
        }
        if !same_set(&m.permissions, accepted) {
            bail!("I permessi di {} sono cambiati: controllali di nuovo prima di installare", m.name);
        }
        if self.records().get(id).is_some_and(|r| r.dev_path.is_some()) {
            bail!("{} è caricato in modalità sviluppatore: toglilo prima di installare quello del catalogo", m.name);
        }
        if entry.size > MAX_PACKAGE {
            bail!("Il pacchetto di {} è troppo grande", m.name);
        }
        let data = self.fetch(&entry.file, MAX_PACKAGE).await?;
        if data.len() as u64 != entry.size || hex(&Sha256::digest(&data)) != entry.sha256.to_ascii_lowercase() {
            bail!("Il pacchetto di {} non corrisponde al catalogo: scartato", m.name);
        }

        let dir = self.root.join(id);
        let tmp = dir.join(format!("{}.tmp", m.version));
        let target = dir.join(&m.version);
        let unpack = tmp.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            if unpack.exists() {
                std::fs::remove_dir_all(&unpack)?;
            }
            std::fs::create_dir_all(&unpack)?;
            extract_package(&data, &unpack)
        })
        .await?
        .inspect_err(|_| {
            let _ = std::fs::remove_dir_all(&tmp);
        })?;
        match read_manifest(&tmp) {
            Ok(inside) if &inside == m => {}
            Ok(_) => {
                let _ = std::fs::remove_dir_all(&tmp);
                bail!("Il pacchetto di {} non corrisponde al catalogo: scartato", m.name);
            }
            Err(e) => {
                let _ = std::fs::remove_dir_all(&tmp);
                return Err(e.context(format!("Pacchetto di {} non valido", m.name)));
            }
        }
        if target.exists() {
            std::fs::remove_dir_all(&target)?;
        }
        std::fs::rename(&tmp, &target)?;

        {
            let mut records = self.records();
            let enabled = records.get(id).is_none_or(|r| r.enabled);
            records.insert(
                id.to_string(),
                Record { version: m.version.clone(), enabled, granted: accepted.to_vec(), dev_path: None },
            );
            self.save(&records)?;
        }
        // Le versioni precedenti non servono più (i dati stanno in data.json, fuori).
        for old in std::fs::read_dir(&dir)?.flatten() {
            if old.path().is_dir() && old.file_name() != m.version.as_str() {
                let _ = std::fs::remove_dir_all(old.path());
            }
        }
        self.installed();
        Ok(())
    }

    /// Toglie l'add-on e cancella i suoi dati. Di un add-on in sviluppo si
    /// cancellano solo i dati: la cartella del sorgente resta com'è.
    pub fn uninstall(&self, id: &str) -> Result<()> {
        {
            let mut records = self.records();
            if records.remove(id).is_none() {
                bail!("{id} non è installato");
            }
            self.save(&records)?;
        }
        self.installed();
        let dir = self.root.join(id);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).with_context(|| format!("File di {id} non cancellati: riprova"))?;
        }
        Ok(())
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        {
            let mut records = self.records();
            let rec = records.get_mut(id).ok_or_else(|| anyhow!("{id} non è installato"))?;
            rec.enabled = enabled;
            self.save(&records)?;
        }
        self.installed();
        Ok(())
    }

    /// Modalità sviluppatore: usa l'add-on direttamente dalla sua cartella,
    /// senza catalogo né firma. Restituisce l'id.
    pub fn add_dev(&self, path: &Path) -> Result<String> {
        let path = std::path::absolute(path)?;
        let m = read_manifest(&path)?;
        {
            let mut records = self.records();
            if records.get(&m.id).is_some_and(|r| r.dev_path.is_none()) {
                bail!("{} è già installato dal catalogo: disinstallalo prima di caricarlo in sviluppo", m.name);
            }
            records.insert(
                m.id.clone(),
                Record { version: m.version.clone(), enabled: true, granted: m.permissions.clone(), dev_path: Some(path) },
            );
            self.save(&records)?;
        }
        self.installed();
        Ok(m.id)
    }

    /// L'add-on è attivo e, se indicato, ha il permesso.
    pub fn require(&self, id: &str, permission: Option<&str>) -> Result<()> {
        let records = self.records();
        let rec = records.get(id).filter(|r| r.enabled).ok_or_else(|| anyhow!("L'add-on non è attivo"))?;
        if let Some(p) = permission
            && !rec.granted.iter().any(|g| g == p)
        {
            bail!("L'add-on non ha il permesso «{p}»");
        }
        Ok(())
    }

    /// Dati dell'add-on (JSON, al massimo 1 MB in tutto).
    pub fn storage(&self, id: &str, op: StorageOp) -> Result<Value> {
        self.require(id, None)?;
        let _guard = self.storage_lock.lock().unwrap_or_else(|p| p.into_inner());
        let path = self.root.join(id).join("data.json");
        let mut data: BTreeMap<String, Value> = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let key_ok = |k: &str| -> Result<()> {
            if k.is_empty() || k.chars().count() > 100 {
                bail!("Chiave dei dati non valida: da 1 a 100 caratteri");
            }
            Ok(())
        };
        match op {
            StorageOp::Get(k) => Ok(data.get(&k).cloned().unwrap_or(Value::Null)),
            StorageOp::Keys => Ok(Value::from(data.keys().cloned().collect::<Vec<_>>())),
            StorageOp::Set(k, v) => {
                key_ok(&k)?;
                data.insert(k, v);
                let bytes = serde_json::to_vec(&data)?;
                if bytes.len() > MAX_STORAGE {
                    bail!("Spazio dell'add-on pieno (1 MB)");
                }
                std::fs::create_dir_all(path.parent().unwrap_or(&self.root))?;
                write_atomic(&path, &bytes)?;
                Ok(Value::Null)
            }
            StorageOp::Remove(k) => {
                if data.remove(&k).is_some() {
                    write_atomic(&path, &serde_json::to_vec(&data)?)?;
                }
                Ok(Value::Null)
            }
        }
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

// ---- server locale ----

type Resp = tiny_http::Response<Box<dyn Read + Send>>;

fn handle(shared: &Shared, request: tiny_http::Request) {
    let response = route(shared, &request).unwrap_or_else(status_response);
    let _ = request.respond(response);
}

fn route(shared: &Shared, request: &tiny_http::Request) -> std::result::Result<Resp, u16> {
    // Solo richieste indirizzate a noi: un sito esterno che punta un suo nome
    // a 127.0.0.1 (DNS rebinding) non passa.
    let expected = [format!("127.0.0.1:{}", shared.port), format!("localhost:{}", shared.port)];
    let host_ok = request
        .headers()
        .iter()
        .any(|h| h.field.equiv("Host") && expected.iter().any(|e| h.value.as_str() == e));
    if !host_ok || *request.method() != tiny_http::Method::Get {
        return Err(403);
    }
    let path = request.url().split(['?', '#']).next().unwrap_or("/");
    let mut parts = path.strip_prefix("/a/").ok_or(404u16)?.splitn(3, '/');
    let (id, mount, file) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let site = shared.sites.lock().unwrap_or_else(|p| p.into_inner()).get(id).cloned().ok_or(404u16)?;
    if site.mount != mount {
        return Err(404);
    }
    let (data, mime) = match file {
        "_postamon/sdk.js" => (SDK_JS.as_bytes().to_vec(), "text/javascript"),
        "_postamon/postamon.css" => (SDK_CSS.as_bytes().to_vec(), "text/css"),
        _ => {
            package_path(file).map_err(|_| 403u16)?;
            let full = site.dir.join(file);
            (std::fs::read(&full).map_err(|_| 404u16)?, crate::craft::mime_for(&full))
        }
    };
    let len = data.len();
    let reader: Box<dyn Read + Send> = Box::new(std::io::Cursor::new(data));
    let mut resp = tiny_http::Response::new(200.into(), vec![], reader, Some(len), None);
    let cache = if site.dev { "no-store" } else { "no-cache" };
    for (k, v) in [
        ("Content-Type", mime),
        ("Cache-Control", cache),
        ("X-Content-Type-Options", "nosniff"),
        ("Referrer-Policy", "no-referrer"),
        // Il riquadro ha un'origine opaca: senza questa intestazione non
        // potrebbe leggere i suoi stessi file (moduli, fetch, font).
        ("Access-Control-Allow-Origin", "*"),
        // Su tutte le risposte, non solo sull'HTML: un worker prende la
        // regola dalla risposta del suo script.
        ("Content-Security-Policy", site.csp.as_str()),
    ] {
        if let Ok(h) = tiny_http::Header::from_bytes(k.as_bytes(), v.as_bytes()) {
            resp.add_header(h);
        }
    }
    Ok(resp)
}

fn status_response(status: u16) -> Resp {
    let body = status.to_string().into_bytes();
    let len = body.len();
    let reader: Box<dyn Read + Send> = Box::new(std::io::Cursor::new(body));
    tiny_http::Response::new(status.into(), vec![], reader, Some(len), None)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    /// Chiave di prova che ha firmato `testdata/addons/addons.json`. Per
    /// rigenerare il catalogo di prova: `npx tauri signer generate`, poi
    /// `node scripts/addons.mjs pack src-tauri/testdata/addons`, la firma con
    /// la nuova chiave e la sua parte pubblica qui.
    const TEST_PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEQ2N0M3QkREM0UwM0U1NkYKUldSdjVRTSszWHQ4MWs0ekc3RytJekorbm5ONGhqUXhoaDZUVThRVDVKWndvUi9JT1p3azFpRkQK";
    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/addons");
    const EXAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../addons/modelli-risposta");

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("postamon-addons-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Serve la cartella del catalogo di prova; `tamper` aggiunge un byte a
    /// quel file, come un pacchetto manomesso.
    fn serve(tamper: Option<&'static str>) -> String {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        std::thread::spawn(move || {
            for request in server.incoming_requests() {
                let name = request.url().trim_start_matches('/').to_string();
                let response = match std::fs::read(Path::new(FIXTURES).join(&name)) {
                    Ok(mut data) => {
                        if tamper == Some(name.as_str()) {
                            data.push(b'!');
                        }
                        tiny_http::Response::from_data(data)
                    }
                    Err(_) => tiny_http::Response::from_data(Vec::new()).with_status_code(404),
                };
                let _ = request.respond(response);
            }
        });
        format!("http://127.0.0.1:{port}/")
    }

    fn example() -> Manifest {
        serde_json::from_slice(&std::fs::read(Path::new(EXAMPLE).join("addon.json")).unwrap()).unwrap()
    }

    #[test]
    fn the_example_addon_is_valid() {
        let m = read_manifest(Path::new(EXAMPLE)).unwrap();
        assert_eq!(m.id, "modelli-risposta");
        assert!(m.incompatible().is_none());
    }

    #[test]
    fn rejects_bad_manifests() {
        let bad = |change: &dyn Fn(&mut Manifest)| {
            let mut m = example();
            change(&mut m);
            m.validate().unwrap_err().to_string()
        };
        assert!(bad(&|m| m.id = "Maiuscole".into()).contains("id"));
        assert!(bad(&|m| m.id = "-trattino".into()).contains("id"));
        assert!(bad(&|m| m.version = "1.0".into()).contains("version"));
        assert!(bad(&|m| m.permissions.push("password.read".into())).contains("sconosciuto"));
        assert!(bad(&|m| m.permissions.push("compose".into())).contains("ripetuto"));
        assert!(bad(&|m| m.permissions.push("network:192.168.1.10".into())).contains("dominio"));
        assert!(bad(&|m| m.permissions.push("network:*.esempio.it".into())).contains("dominio"));
        assert!(bad(&|m| m.permissions.push("network:esempio.it:8080".into())).contains("dominio"));
        assert!(bad(&|m| m.hooks[0].page = "../fuori.html".into()).contains("percorso"));
        assert!(bad(&|m| m.hooks[0].page = "_postamon/sdk.html".into()).contains("riservata"));
        assert!(bad(&|m| m.hooks[0].page = "script.js".into()).contains(".html"));
        assert!(bad(&|m| m.hooks[0].kind = "toolbar".into()).contains("sconosciuto"));
        assert!(bad(&|m| m.hooks.clear()).contains("hooks"));
        let mut newer_api = example();
        newer_api.api = API_VERSION + 1;
        assert!(newer_api.validate().is_ok());
        assert!(newer_api.incompatible().unwrap().contains("aggiorna Postamon"));
    }

    #[test]
    fn package_paths() {
        assert!(package_path("index.html").is_ok());
        assert!(package_path("img/logo.svg").is_ok());
        for p in ["", "/assoluto.html", "a/../b.html", "a\\b.html", "C:x.html", ".nascosto.js", "a//b.js", "file.exe", "spazio x.html", "x%2e.html"] {
            assert!(package_path(p).is_err(), "{p} doveva essere rifiutato");
        }
    }

    #[test]
    fn csp_only_opens_declared_hosts() {
        let none = csp("http://127.0.0.1:5", "prova", &["message.read".into()]);
        assert!(none.contains("connect-src http://127.0.0.1:5/a/prova/;"));
        assert!(none.contains("default-src 'none'"));
        assert!(!none.contains("https://"));
        let net = csp("http://127.0.0.1:5", "prova", &["network:api.esempio.it".into()]);
        assert!(net.contains("connect-src http://127.0.0.1:5/a/prova/ https://api.esempio.it;"));
        assert!(net.contains("img-src http://127.0.0.1:5/a/prova/ data: blob: https://api.esempio.it;"));
    }

    #[test]
    fn verifies_the_catalog_signature() {
        let json = std::fs::read(Path::new(FIXTURES).join("addons.json")).unwrap();
        let sig = std::fs::read_to_string(Path::new(FIXTURES).join("addons.json.sig")).unwrap();
        verify_signature(TEST_PUBKEY, &json, &sig).unwrap();
        let mut changed = json.clone();
        changed.push(b' ');
        assert!(verify_signature(TEST_PUBKEY, &changed, &sig).is_err());
        assert!(verify_signature(PUBKEY, &json, &sig).is_err(), "firmato con un'altra chiave");
        assert!(verify_signature(TEST_PUBKEY, &json, "non è una firma").is_err());
    }

    #[test]
    fn extraction_refuses_dangerous_entries() {
        let zip_with = |name: &str| {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            w.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(b"x").unwrap();
            w.finish().unwrap().into_inner()
        };
        let dir = temp("estrazione");
        for name in ["../fuori.html", "virus.exe", "_postamon/sdk.js", "a/.nascosto.js"] {
            assert!(extract_package(&zip_with(name), &dir).is_err(), "{name} doveva essere rifiutato");
        }
        assert!(!dir.parent().unwrap().join("fuori.html").exists());
        extract_package(&zip_with("pagine/index.html"), &dir).unwrap();
        assert!(dir.join("pagine/index.html").is_file());
        let _ = std::fs::remove_dir_all(dir);
    }

    async fn get(url: &str, host: Option<&str>) -> (u16, String, String) {
        let client = reqwest::Client::new();
        let mut req = client.get(url);
        if let Some(h) = host {
            req = req.header("Host", h);
        }
        let res = req.send().await.unwrap();
        let status = res.status().as_u16();
        let csp = res.headers().get("content-security-policy").map(|v| v.to_str().unwrap().to_string()).unwrap_or_default();
        (status, csp, res.text().await.unwrap())
    }

    #[tokio::test]
    async fn installs_serves_and_uninstalls() {
        let root = temp("installa");
        let addons = Addons::with(root.clone(), serve(None), TEST_PUBKEY.into()).unwrap();
        let items = addons.catalog_items().await.unwrap();
        let item = items.iter().find(|i| i.entry.manifest.id == "modelli-risposta").expect("nel catalogo");
        assert_eq!(item.installed_version, None);
        assert_eq!(item.new_permissions, item.entry.manifest.permissions);
        let perms = item.entry.manifest.permissions.clone();

        // Permessi diversi da quelli del catalogo: niente installazione.
        let err = addons.install("modelli-risposta", &["compose".into()]).await.unwrap_err().to_string();
        assert!(err.contains("permessi"), "{err}");
        addons.install("modelli-risposta", &perms).await.unwrap();

        let list = addons.installed();
        let inst = list.iter().find(|a| a.id == "modelli-risposta").unwrap();
        assert!(inst.enabled && !inst.dev && inst.error.is_none());
        let page = &inst.manifest.as_ref().unwrap().hooks[0].page;
        let (status, csp, body) = get(&format!("{}{page}", inst.base_url), None).await;
        assert_eq!(status, 200);
        assert!(body.contains("_postamon/sdk.js"));
        assert!(csp.contains("default-src 'none'"));
        let (status, _, sdk) = get(&format!("{}_postamon/sdk.js", inst.base_url), None).await;
        assert_eq!(status, 200);
        assert!(sdk.contains("window.parent.postMessage"));
        assert_eq!(get(&format!("{}../../../installed.json", inst.base_url), None).await.0, 404);
        assert_eq!(get(&format!("{}addon.json", inst.base_url.replace("/1.", "/9.")), None).await.0, 404);
        let wrong_host = format!("evil.example:{}", addons.shared.port);
        assert_eq!(get(&format!("{}{page}", inst.base_url), Some(&wrong_host)).await.0, 403);

        // Dati dell'add-on.
        addons.storage("modelli-risposta", StorageOp::Set("modelli".into(), serde_json::json!([1, 2]))).unwrap();
        assert_eq!(addons.storage("modelli-risposta", StorageOp::Get("modelli".into())).unwrap(), serde_json::json!([1, 2]));
        assert_eq!(addons.storage("modelli-risposta", StorageOp::Keys).unwrap(), serde_json::json!(["modelli"]));
        let big = Value::String("x".repeat(MAX_STORAGE));
        assert!(addons.storage("modelli-risposta", StorageOp::Set("tanto".into(), big)).is_err());
        assert!(addons.require("modelli-risposta", Some("message.read")).is_ok());
        assert!(addons.require("modelli-risposta", Some("network:esempio.it")).is_err());

        // Disattivato: niente pagine né dati.
        addons.set_enabled("modelli-risposta", false).unwrap();
        assert_eq!(get(&format!("{}{page}", inst.base_url), None).await.0, 404);
        assert!(addons.storage("modelli-risposta", StorageOp::Keys).is_err());
        addons.set_enabled("modelli-risposta", true).unwrap();

        // Gli add-on installati restano dopo un riavvio.
        let again = Addons::with(root.clone(), serve(None), TEST_PUBKEY.into()).unwrap();
        assert_eq!(again.installed().len(), 1);

        addons.uninstall("modelli-risposta").unwrap();
        assert!(addons.installed().is_empty());
        assert!(!root.join("modelli-risposta").exists());
        assert_eq!(get(&format!("{}{page}", inst.base_url), None).await.0, 404);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn refuses_tampered_packages() {
        let root = temp("manomesso");
        let catalog: serde_json::Value =
            serde_json::from_slice(&std::fs::read(Path::new(FIXTURES).join("addons.json")).unwrap()).unwrap();
        let file: &'static str = catalog["addons"][0]["file"].as_str().unwrap().to_string().leak();
        let addons = Addons::with(root.clone(), serve(Some(file)), TEST_PUBKEY.into()).unwrap();
        let perms = example().permissions;
        let err = addons.install("modelli-risposta", &perms).await.unwrap_err().to_string();
        assert!(err.contains("non corrisponde"), "{err}");
        assert!(addons.installed().is_empty());

        let signed_by_other = Addons::with(root.clone(), serve(None), PUBKEY.into()).unwrap();
        let err = signed_by_other.catalog_items().await.unwrap_err().to_string();
        assert!(err.contains("firma"), "{err}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn developer_mode_serves_the_source_folder() {
        let root = temp("sviluppo");
        let addons = Addons::with(root.clone(), serve(None), TEST_PUBKEY.into()).unwrap();
        let id = addons.add_dev(Path::new(EXAMPLE)).unwrap();
        let inst = addons.installed().into_iter().find(|a| a.id == id).unwrap();
        assert!(inst.dev && inst.base_url.ends_with("/dev/"));
        let page = &inst.manifest.as_ref().unwrap().hooks[0].page;
        assert_eq!(get(&format!("{}{page}", inst.base_url), None).await.0, 200);
        // Lo stesso id dal catalogo non si installa sopra quello in sviluppo.
        let err = addons.install(&id, &example().permissions).await.unwrap_err().to_string();
        assert!(err.contains("sviluppatore"), "{err}");
        // Toglierlo non tocca la cartella del sorgente.
        addons.uninstall(&id).unwrap();
        assert!(Path::new(EXAMPLE).join("addon.json").is_file());
        let _ = std::fs::remove_dir_all(root);
    }
}

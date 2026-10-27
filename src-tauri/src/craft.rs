//! Integrazione con le app ArtCraft (github.com/storytold, MIT/Apache-2.0).
//!
//! **Anteprima dentro Postamon.** Ogni app Craft pubblica nelle release GitHub
//! un `<app>-web-<versione>.zip`, la stessa app compilata per il browser e
//! pensata per stare in un iframe. Postamon lo scarica la prima volta che
//! serve, controlla lo SHA-256 con il `SHA256SUMS.txt` della release e lo
//! tiene in `<dati>/craft/<app>/`. Un piccolo server HTTP su 127.0.0.1 serve
//! il sito e l'allegato dalla stessa origine; è un'origine diversa da quella
//! dell'interfaccia, quindi l'app Craft non ha accesso ai comandi di Postamon.
//! PdfCraft apre il file con il suo parametro `?file=`; alle altre uno script
//! aggiunto alla pagina lascia cadere l'allegato come un file trascinato.
//!
//! **App installate.** Se sul PC mancano le app ArtCraft, Postamon propone di
//! installarle tutte: scarica gli MSI ufficiali (stesso controllo SHA-256) e
//! li installa in silenzio con `INSTALLDESKTOPSHORTCUT=0`, cioè senza icone
//! sul desktop. Il launcher si installa per l'utente; le app, che vanno in
//! Programmi, tutte insieme con una sola conferma di amministratore.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ---- catalogo ----

/// App che sa mostrare un tipo di allegato.
pub struct CraftApp {
    pub id: &'static str,
    pub name: &'static str,
    exts: &'static [&'static str],
    /// L'app apre da sola il documento indicato con `?file=<url>`.
    file_param: bool,
}

pub const VIEWERS: &[CraftApp] = &[
    CraftApp { id: "pdfcraft", name: "PdfCraft", exts: &["pdf"], file_param: true },
    CraftApp {
        id: "photocraft",
        name: "PhotoCraft",
        exts: &[
            "png", "jpg", "jpeg", "jfif", "gif", "webp", "bmp", "tif", "tiff", "heic", "heif", "psd", "psb", "tga",
            "ico", "qoi", "exr", "hdr", "xcf", "kra", "ora", "pdn", "afphoto",
        ],
        file_param: false,
    },
    CraftApp {
        id: "wordcraft",
        name: "WordCraft",
        exts: &["docx", "doc", "odt", "rtf", "md", "tex", "txt"],
        file_param: false,
    },
    CraftApp {
        id: "gridcraft",
        name: "GridCraft",
        exts: &["xlsx", "xlsm", "xls", "ods", "csv", "tsv", "numbers"],
        file_param: false,
    },
    CraftApp { id: "deckcraft", name: "DeckCraft", exts: &["pptx"], file_param: false },
    CraftApp { id: "vectorcraft", name: "VectorCraft", exts: &["svg", "ai", "eps", "afdesign"], file_param: false },
    CraftApp {
        id: "soundcraft",
        name: "SoundCraft",
        exts: &["wav", "mp3", "flac", "ogg", "aif", "aiff", "m4a"],
        file_param: false,
    },
    CraftApp {
        id: "filmcraft",
        name: "FilmCraft",
        exts: &["mp4", "m4v", "mov", "webm", "avi", "mxf"],
        file_param: false,
    },
    CraftApp { id: "cadcraft", name: "CADCraft", exts: &["dxf", "dwg"], file_param: false },
];

pub fn viewer_for(file_name: &str) -> Option<&'static CraftApp> {
    let ext = Path::new(file_name).extension()?.to_str()?.to_ascii_lowercase();
    VIEWERS.iter().find(|a| a.exts.contains(&ext.as_str()))
}

fn viewer_by_id(id: &str) -> Option<&'static CraftApp> {
    VIEWERS.iter().find(|a| a.id == id)
}

/// Un programma della suite da installare sul PC.
pub struct SuiteApp {
    pub id: &'static str,
    pub name: &'static str,
    repo: &'static str,
    /// Il launcher si installa per l'utente, le app per tutto il PC.
    per_user: bool,
}

const fn app(id: &'static str, name: &'static str, repo: &'static str) -> SuiteApp {
    SuiteApp { id, name, repo, per_user: false }
}

pub const SUITE: &[SuiteApp] = &[
    SuiteApp { id: "artcraft-launcher", name: "ArtCraft Launcher", repo: "craft-launcher", per_user: true },
    app("pdfcraft", "PdfCraft", "pdfcraft"),
    app("photocraft", "PhotoCraft", "photocraft"),
    app("wordcraft", "WordCraft", "wordcraft"),
    app("gridcraft", "GridCraft", "gridcraft"),
    app("deckcraft", "DeckCraft", "deckcraft"),
    app("vectorcraft", "VectorCraft", "vectorcraft"),
    app("soundcraft", "SoundCraft", "soundcraft"),
    app("filmcraft", "FilmCraft", "filmcraft"),
    app("cadcraft", "CADCraft", "cadcraft"),
    app("designcraft", "DesignCraft", "designcraft"),
    app("effectcraft", "EffectCraft", "effectcraft"),
    app("lightcraft", "LightCraft", "lightcraft"),
];

fn native_exe(app: &SuiteApp) -> Option<PathBuf> {
    if app.per_user {
        let local = PathBuf::from(std::env::var_os("LOCALAPPDATA")?);
        Some(local.join("Programs").join(app.name).join(format!("{}.exe", app.id)))
    } else {
        let programs = std::env::var_os("ProgramW6432").or_else(|| std::env::var_os("ProgramFiles"))?;
        Some(PathBuf::from(programs).join(app.name).join(format!("{}.exe", app.id)))
    }
}

/// Programmi della suite che non risultano installati.
pub fn missing_suite() -> Vec<&'static SuiteApp> {
    SUITE.iter().filter(|a| !native_exe(a).is_some_and(|p| p.is_file())).collect()
}

/// Eseguibile installato dell'app che mostra questo tipo di file.
pub fn native_viewer(app: &CraftApp) -> Option<PathBuf> {
    let suite = SUITE.iter().find(|s| s.id == app.id)?;
    native_exe(suite).filter(|p| p.is_file())
}

// ---- stato ----

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Installed {
    version: String,
    /// Cartella del sito, relativa a `<dati>/craft/<app>/`.
    site: String,
    checked_at: u64,
}

struct Served {
    mime: String,
    data: Arc<Vec<u8>>,
}

struct Shared {
    root: PathBuf,
    port: u16,
    files: Mutex<HashMap<String, Served>>,
    sites: Mutex<HashMap<String, PathBuf>>,
}

pub struct Craft {
    shared: Arc<Shared>,
    http: reqwest::Client,
    web_lock: tokio::sync::Mutex<()>,
    setup_lock: tokio::sync::Mutex<()>,
}

/// Una nuova versione web si cerca al massimo una volta al giorno.
const UPDATE_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupStep {
    pub app: String,
    /// "download", "install" o "done".
    pub stage: &'static str,
    pub index: usize,
    pub count: usize,
    pub received: u64,
    pub total: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupReport {
    pub installed: Vec<String>,
    pub failed: Vec<String>,
    /// L'utente ha rifiutato la conferma di amministratore.
    pub cancelled: bool,
}

struct Release {
    version: String,
    assets: Vec<(String, String, u64)>,
}

impl Release {
    fn asset(&self, pred: impl Fn(&str) -> bool) -> Option<&(String, String, u64)> {
        self.assets.iter().find(|(name, _, _)| pred(name))
    }
}

impl Craft {
    pub fn start(root: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&root)?;
        let server = tiny_http::Server::http("127.0.0.1:0").map_err(|e| anyhow!("server delle anteprime: {e}"))?;
        let port = server
            .server_addr()
            .to_ip()
            .map(|a| a.port())
            .ok_or_else(|| anyhow!("server delle anteprime senza porta"))?;
        let mut sites = HashMap::new();
        for app in VIEWERS {
            if let Some(inst) = read_installed(&root, app.id) {
                sites.insert(app.id.to_string(), root.join(app.id).join(&inst.site));
            }
        }
        let shared = Arc::new(Shared { root, port, files: Mutex::default(), sites: Mutex::new(sites) });
        let serving = shared.clone();
        std::thread::Builder::new().name("anteprime".into()).spawn(move || {
            for request in server.incoming_requests() {
                let shared = serving.clone();
                std::thread::spawn(move || handle(&shared, request));
            }
        })?;
        let http = reqwest::Client::builder()
            .user_agent(concat!("Postamon/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(20))
            .build()?;
        Ok(Self {
            shared,
            http,
            web_lock: tokio::sync::Mutex::new(()),
            setup_lock: tokio::sync::Mutex::new(()),
        })
    }

    pub fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.shared.port)
    }

    // ---- versioni web, per l'anteprima ----

    pub fn web_installed(&self, app: &CraftApp) -> bool {
        read_installed(&self.shared.root, app.id).is_some()
    }

    pub fn web_needs_check(&self, app: &CraftApp) -> bool {
        read_installed(&self.shared.root, app.id).is_none_or(|i| now() - i.checked_at > UPDATE_EVERY.as_secs())
    }

    /// Installa o aggiorna la versione web dell'app; `progress(ricevuti, totale)`.
    pub async fn install_web(&self, app: &CraftApp, progress: impl Fn(u64, u64) + Send + Sync) -> Result<()> {
        let _guard = self.web_lock.lock().await;
        let root = self.shared.root.join(app.id);
        let release = self.latest_release(app.id).await?;
        let installed = read_installed(&self.shared.root, app.id);
        if let Some(mut inst) = installed.clone().filter(|i| i.version == release.version) {
            inst.checked_at = now();
            write_installed(&root, &inst)?;
            return Ok(());
        }
        let prefix = format!("{}-web-", app.id);
        let (zip_name, _, _) = release
            .asset(|n| n.starts_with(&prefix) && n.ends_with(".zip"))
            .ok_or_else(|| anyhow!("{} {} non ha la versione web", app.name, release.version))?
            .clone();
        std::fs::create_dir_all(&root)?;
        let tmp = root.join("download.tmp");
        self.download_verified(&release, &zip_name, &tmp, &progress).await?;
        let version_dir = root.join(&release.version);
        let (zip, dir) = (tmp.clone(), version_dir.clone());
        tokio::task::spawn_blocking(move || extract(&zip, &dir)).await??;
        let _ = std::fs::remove_file(&tmp);
        let site = find_site(&version_dir)?;
        let inst = Installed {
            version: release.version.clone(),
            site: site.strip_prefix(&root)?.to_string_lossy().into_owned(),
            checked_at: now(),
        };
        write_installed(&root, &inst)?;
        self.shared.sites.lock().unwrap_or_else(|p| p.into_inner()).insert(app.id.to_string(), site);
        if let Some(old) = installed.filter(|i| i.version != release.version) {
            let _ = std::fs::remove_dir_all(root.join(old.version));
        }
        Ok(())
    }

    /// Mette un allegato a disposizione dell'app e restituisce (indirizzo
    /// della pagina, chiave da usare per toglierlo).
    pub fn open(&self, app: &CraftApp, name: &str, mime: &str, data: Vec<u8>) -> (String, String) {
        let token = random_token();
        self.shared
            .files
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(token.clone(), Served { mime: mime.to_string(), data: Arc::new(data) });
        let base = self.origin();
        let url = if app.file_param {
            // PdfCraft mostra come nome l'ultimo pezzo dell'indirizzo senza
            // decodificarlo: niente caratteri da codificare, o comparirebbe "%20".
            let file_url = format!("{base}/file/{token}/{}", plain_name(name));
            format!("{base}/app/{}/?file={}", app.id, encode(&file_url))
        } else {
            let file_url = format!("{base}/file/{token}/{}", encode(name));
            format!("{base}/app/{}/?postamon-file={}&name={}", app.id, encode(&file_url), encode(name))
        };
        (url, token)
    }

    pub fn close(&self, token: &str) {
        self.shared.files.lock().unwrap_or_else(|p| p.into_inner()).remove(token);
    }

    // ---- app installate sul PC ----

    /// Scarica e installa tutti i programmi della suite che mancano.
    pub async fn install_suite(&self, progress: impl Fn(SetupStep) + Send + Sync) -> Result<SetupReport> {
        let _guard = self.setup_lock.lock().await;
        let missing = missing_suite();
        let mut report = SetupReport::default();
        if missing.is_empty() {
            return Ok(report);
        }
        let dir = std::env::temp_dir().join("Postamon").join("artcraft-setup");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        let arch = match std::env::consts::ARCH {
            "aarch64" => "arm64",
            "x86" => "x86",
            _ => "x64",
        };
        let count = missing.len();
        let mut ready: Vec<(&'static SuiteApp, PathBuf)> = Vec::new();
        for (index, app) in missing.iter().enumerate() {
            let step = |stage, received, total| SetupStep {
                app: app.name.to_string(),
                stage,
                index,
                count,
                received,
                total,
            };
            progress(step("download", 0, 0));
            let result = async {
                let release = self.latest_release(app.repo).await?;
                let suffix = format!("-windows-{arch}.msi");
                let (name, _, _) = release
                    .asset(|n| n.starts_with(app.id) && n.ends_with(&suffix))
                    .ok_or_else(|| anyhow!("nessun installer per Windows {arch}"))?
                    .clone();
                let msi = dir.join(&name);
                self.download_verified(&release, &name, &msi, &|r, t| progress(step("download", r, t))).await?;
                Ok::<_, anyhow::Error>(msi)
            }
            .await;
            match result {
                Ok(msi) => ready.push((app, msi)),
                Err(e) => report.failed.push(format!("{}: {e:#}", app.name)),
            }
        }

        let install = SetupStep { app: String::new(), stage: "install", index: count, count, received: 0, total: 0 };
        progress(install.clone());
        // Il launcher va nel profilo dell'utente: niente diritti di amministratore,
        // altrimenti finirebbe nel profilo di chi conferma la richiesta di Windows.
        for (app, msi) in ready.iter().filter(|(a, _)| a.per_user) {
            let msi = msi.clone();
            let code = tokio::task::spawn_blocking(move || msiexec_quiet(&msi)).await??;
            if !msi_ok(code) {
                report.failed.push(format!("{}: installazione non riuscita (codice {code})", app.name));
            }
        }
        let machine: Vec<(&'static SuiteApp, PathBuf)> = ready.iter().filter(|(a, _)| !a.per_user).cloned().collect();
        if !machine.is_empty() {
            let (setup_dir, items) = (dir.clone(), machine.clone());
            match tokio::task::spawn_blocking(move || run_elevated(&setup_dir, &items)).await?? {
                None => report.cancelled = true,
                Some(codes) => {
                    for (app, _) in &machine {
                        match codes.get(app.id) {
                            Some(code) if msi_ok(*code) => {}
                            Some(1618) => report.failed.push(format!(
                                "{}: Windows stava già installando un altro programma, riprova tra poco",
                                app.name
                            )),
                            Some(code) => {
                                report.failed.push(format!("{}: installazione non riuscita (codice {code})", app.name))
                            }
                            None => report.failed.push(format!("{}: installazione non eseguita", app.name)),
                        }
                    }
                }
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
        let still_missing: Vec<&str> = missing_suite().iter().map(|a| a.id).collect();
        report.installed = missing.iter().filter(|a| !still_missing.contains(&a.id)).map(|a| a.name.to_string()).collect();
        progress(SetupStep { stage: "done", ..install });
        Ok(report)
    }

    // ---- GitHub ----

    async fn latest_release(&self, repo: &str) -> Result<Release> {
        let url = format!("https://api.github.com/repos/storytold/{repo}/releases/latest");
        let rel: serde_json::Value = self
            .http
            .get(url)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .context("GitHub non raggiungibile")?
            .error_for_status()
            .context("Elenco delle versioni non disponibile")?
            .json()
            .await?;
        let version = rel["tag_name"].as_str().unwrap_or_default().trim_start_matches('v').to_string();
        let assets = rel["assets"]
            .as_array()
            .map(|list| {
                list.iter()
                    .map(|a| {
                        (
                            a["name"].as_str().unwrap_or_default().to_string(),
                            a["browser_download_url"].as_str().unwrap_or_default().to_string(),
                            a["size"].as_u64().unwrap_or(0),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Release { version, assets })
    }

    /// Scarica un file della release e controlla che corrisponda al suo
    /// SHA-256 pubblicato in `SHA256SUMS.txt`.
    async fn download_verified(
        &self,
        release: &Release,
        name: &str,
        dest: &Path,
        progress: &(dyn Fn(u64, u64) + Send + Sync),
    ) -> Result<()> {
        let (_, sums_url, _) = release
            .asset(|n| n == "SHA256SUMS.txt")
            .ok_or_else(|| anyhow!("release senza SHA256SUMS.txt"))?;
        let sums = self.http.get(sums_url).send().await?.error_for_status()?.text().await?;
        let expected = sums
            .lines()
            .filter_map(|l| l.split_once(char::is_whitespace))
            .find(|(_, file)| file.trim().trim_start_matches('*') == name)
            .map(|(hash, _)| hash.to_ascii_lowercase())
            .ok_or_else(|| anyhow!("{name} non ha il controllo di integrità"))?;
        let (_, url, size) = release.asset(|n| n == name).ok_or_else(|| anyhow!("{name} non trovato"))?;
        let response = self.http.get(url).send().await?.error_for_status()?;
        let total = response.content_length().unwrap_or(*size);
        let mut file = std::fs::File::create(dest)?;
        let mut hasher = Sha256::new();
        let mut received = 0u64;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("download interrotto")?;
            hasher.update(&chunk);
            file.write_all(&chunk)?;
            received += chunk.len() as u64;
            progress(received, total);
        }
        drop(file);
        let actual: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if actual != expected {
            let _ = std::fs::remove_file(dest);
            bail!("{name} scaricato non corrisponde a quello pubblicato: scartato");
        }
        Ok(())
    }
}

fn msi_ok(code: i32) -> bool {
    // 3010 e 1641: installato, serve un riavvio.
    matches!(code, 0 | 3010 | 1641)
}

#[cfg(windows)]
fn hidden(cmd: &mut std::process::Command) -> &mut std::process::Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW)
}

#[cfg(not(windows))]
fn hidden(cmd: &mut std::process::Command) -> &mut std::process::Command {
    cmd
}

fn msiexec_quiet(msi: &Path) -> Result<i32> {
    let status = hidden(std::process::Command::new("msiexec.exe").arg("/i").arg(msi).args([
        "/qn",
        "/norestart",
        "INSTALLDESKTOPSHORTCUT=0",
    ]))
    .status()
    .context("avvio di msiexec")?;
    Ok(status.code().unwrap_or(-1))
}

/// Testo fra apici singoli per PowerShell.
fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Installa gli MSI con una sola richiesta di amministratore. `None` se
/// l'utente ha rifiutato la richiesta.
fn run_elevated(dir: &Path, items: &[(&'static SuiteApp, PathBuf)]) -> Result<Option<HashMap<String, i32>>> {
    let results = dir.join("risultati.txt");
    let mut script = String::from("$ErrorActionPreference = 'Continue'\r\n");
    for (app, msi) in items {
        script.push_str(&format!(
            "$p = Start-Process -FilePath 'msiexec.exe' -Wait -PassThru -ArgumentList @('/i', {}, '/qn', '/norestart', 'INSTALLDESKTOPSHORTCUT=0')\r\n\
             Add-Content -LiteralPath {} -Value ({} + ' ' + $p.ExitCode)\r\n",
            ps_quote(&format!("\"{}\"", msi.display())),
            ps_quote(&results.display().to_string()),
            ps_quote(app.id),
        ));
    }
    let install = dir.join("installa.ps1");
    std::fs::write(&install, script)?;
    let elevate = dir.join("conferma.ps1");
    std::fs::write(
        &elevate,
        format!(
            "try {{\r\n  Start-Process -FilePath 'powershell.exe' -Verb RunAs -Wait -WindowStyle Hidden \
             -ArgumentList @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', {})\r\n  exit 0\r\n}} catch {{ exit 5 }}\r\n",
            ps_quote(&format!("\"{}\"", install.display()))
        ),
    )?;
    let status = hidden(
        std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&elevate),
    )
    .status()
    .context("richiesta di amministratore")?;
    if status.code() == Some(5) {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&results).unwrap_or_default();
    Ok(Some(
        text.lines()
            .filter_map(|l| {
                let (id, code) = l.trim().split_once(' ')?;
                Some((id.to_string(), code.trim().parse().ok()?))
            })
            .collect(),
    ))
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn random_token() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("generatore casuale del sistema");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Nome di file che sta in un indirizzo così com'è: "Città del preventivo.pdf"
/// → "Citta_del_preventivo.pdf".
fn plain_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ä' => 'a',
            'À' | 'Á' | 'Â' | 'Ä' => 'A',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'È' | 'É' | 'Ê' | 'Ë' => 'E',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'Ì' | 'Í' | 'Î' | 'Ï' => 'I',
            'ò' | 'ó' | 'ô' | 'ö' => 'o',
            'Ò' | 'Ó' | 'Ô' | 'Ö' => 'O',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            'Ù' | 'Ú' | 'Û' | 'Ü' => 'U',
            c if c.is_ascii_alphanumeric() || "-_.~()!,'+".contains(c) => c,
            _ => '_',
        })
        .collect()
}

fn read_installed(root: &Path, id: &str) -> Option<Installed> {
    let inst: Installed = serde_json::from_slice(&std::fs::read(root.join(id).join("current.json")).ok()?).ok()?;
    root.join(id).join(&inst.site).join("index.html").is_file().then_some(inst)
}

fn write_installed(app_root: &Path, inst: &Installed) -> Result<()> {
    std::fs::write(app_root.join("current.json"), serde_json::to_vec_pretty(inst)?)?;
    Ok(())
}

fn extract(zip_path: &Path, dest: &Path) -> Result<()> {
    if dest.exists() {
        std::fs::remove_dir_all(dest)?;
    }
    std::fs::create_dir_all(dest)?;
    let mut archive = zip::ZipArchive::new(std::fs::File::open(zip_path)?)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        // enclosed_name scarta i percorsi che uscirebbero dalla cartella.
        let Some(rel) = entry.enclosed_name() else { continue };
        let out = dest.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
        } else {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::io::copy(&mut entry, &mut std::fs::File::create(&out)?)?;
        }
    }
    Ok(())
}

/// La cartella del pacchetto che contiene index.html (di solito `<app>-web-<versione>/`).
fn find_site(dir: &Path) -> Result<PathBuf> {
    if dir.join("index.html").is_file() {
        return Ok(dir.to_path_buf());
    }
    for entry in std::fs::read_dir(dir)?.flatten() {
        if entry.path().join("index.html").is_file() {
            return Ok(entry.path());
        }
    }
    bail!("pacchetto senza index.html")
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
    if let Some(rest) = path.strip_prefix("/file/") {
        let token = rest.split('/').next().unwrap_or("");
        let files = shared.files.lock().unwrap_or_else(|p| p.into_inner());
        let served = files.get(token).ok_or(404u16)?;
        let len = served.data.len();
        let reader: Box<dyn Read + Send> = Box::new(ArcReader { data: served.data.clone(), pos: 0 });
        return Ok(with_headers(
            tiny_http::Response::new(200.into(), vec![], reader, Some(len), None),
            &served.mime,
            "no-store",
        ));
    }
    let rest = path.strip_prefix("/app/").ok_or(404u16)?;
    let (id, file) = rest.split_once('/').unwrap_or((rest, ""));
    let app = viewer_by_id(id).ok_or(404u16)?;
    let site = shared.sites.lock().unwrap_or_else(|p| p.into_inner()).get(id).cloned().ok_or(404u16)?;
    let file = if file.is_empty() { "index.html" } else { file };
    let rel = Path::new(file);
    if rel.components().any(|c| !matches!(c, Component::Normal(_))) || file.contains(['\\', ':']) {
        return Err(403);
    }
    let full = site.join(rel);
    let mut data = std::fs::read(&full).map_err(|_| 404u16)?;
    if file == "index.html" && !app.file_param {
        data = inject_bridge(&String::from_utf8_lossy(&data)).into_bytes();
    }
    let cache = if file == "index.html" || file.ends_with("parts.txt") { "no-cache" } else { "max-age=31536000" };
    let len = data.len();
    let reader: Box<dyn Read + Send> = Box::new(std::io::Cursor::new(data));
    Ok(with_headers(tiny_http::Response::new(200.into(), vec![], reader, Some(len), None), mime_for(&full), cache))
}

fn with_headers(mut resp: Resp, mime: &str, cache: &str) -> Resp {
    for (k, v) in [("Content-Type", mime), ("Cache-Control", cache), ("X-Content-Type-Options", "nosniff")] {
        if let Ok(h) = tiny_http::Header::from_bytes(k.as_bytes(), v.as_bytes()) {
            resp.add_header(h);
        }
    }
    resp
}

fn status_response(status: u16) -> Resp {
    let body = status.to_string().into_bytes();
    let len = body.len();
    let reader: Box<dyn Read + Send> = Box::new(std::io::Cursor::new(body));
    tiny_http::Response::new(status.into(), vec![], reader, Some(len), None)
}

pub(crate) fn mime_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript",
        "wasm" => "application/wasm",
        "css" => "text/css",
        "json" => "application/json",
        "txt" => "text/plain; charset=utf-8",
        "md" => "text/markdown; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        _ => "application/octet-stream",
    }
}

struct ArcReader {
    data: Arc<Vec<u8>>,
    pos: usize,
}

impl Read for ArcReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = buf.len().min(self.data.len() - self.pos);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// Per le app senza `?file=`: scarica l'allegato e, appena l'app accetta i
/// file trascinati (il suo `dragover` viene accettato), lo lascia cadere
/// sulla sua area di lavoro. Avvisa Postamon con postMessage.
const BRIDGE: &str = r#"<script>
(() => {
  const params = new URLSearchParams(location.search);
  const src = params.get("postamon-file");
  if (!src) return;
  const name = params.get("name") || "file";
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const tell = (state) => { try { parent.postMessage({ postamon: state }, "*"); } catch (_) {} };
  (async () => {
    const blob = await (await fetch(src)).blob();
    const file = new File([blob], name, { type: blob.type });
    for (let i = 0; i < 1200; i++) {
      const target = document.querySelector("canvas");
      if (target && target.clientWidth > 0) {
        const dt = new DataTransfer();
        dt.items.add(file);
        const probe = new DragEvent("dragover", { dataTransfer: dt, bubbles: true, cancelable: true });
        target.dispatchEvent(probe);
        if (probe.defaultPrevented) {
          await sleep(250);
          target.dispatchEvent(new DragEvent("dragenter", { dataTransfer: dt, bubbles: true, cancelable: true }));
          target.dispatchEvent(new DragEvent("drop", { dataTransfer: dt, bubbles: true, cancelable: true }));
          tell("opened");
          return;
        }
      }
      await sleep(100);
    }
    tell("timeout");
  })().catch(() => tell("error"));
})();
</script>"#;

fn inject_bridge(html: &str) -> String {
    match html.rfind("</body>") {
        Some(i) => format!("{}{BRIDGE}{}", &html[..i], &html[i..]),
        None => format!("{html}{BRIDGE}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_viewer_by_extension() {
        assert_eq!(viewer_for("Preventivo Ergoluna.PDF").map(|a| a.id), Some("pdfcraft"));
        assert_eq!(viewer_for("foto.jpeg").map(|a| a.id), Some("photocraft"));
        assert_eq!(viewer_for("turni.xlsx").map(|a| a.id), Some("gridcraft"));
        assert_eq!(viewer_for("contratto.docx").map(|a| a.id), Some("wordcraft"));
        assert_eq!(viewer_for("archivio.zip").map(|a| a.id), None);
        assert_eq!(viewer_for("senza-estensione").map(|a| a.id), None);
    }

    #[test]
    fn every_viewer_is_in_the_suite() {
        for v in VIEWERS {
            assert!(SUITE.iter().any(|s| s.id == v.id), "{} manca nella suite", v.id);
        }
    }

    #[test]
    fn encodes_urls() {
        assert_eq!(encode("Turni novembre 2026.pdf"), "Turni%20novembre%202026.pdf");
        assert_eq!(encode("http://127.0.0.1:5/f/a"), "http%3A%2F%2F127.0.0.1%3A5%2Ff%2Fa");
    }

    #[test]
    fn plain_names_need_no_encoding() {
        assert_eq!(plain_name("Città del preventivo (v2).pdf"), "Citta_del_preventivo_(v2).pdf");
        assert_eq!(encode(&plain_name("Perché sì.pdf")), "Perche_si.pdf");
    }

    #[test]
    fn quotes_for_powershell() {
        assert_eq!(ps_quote("C:\\Users\\D'Amico\\x.msi"), "'C:\\Users\\D''Amico\\x.msi'");
    }

    #[test]
    fn bridge_goes_before_body_end() {
        let out = inject_bridge("<html><body><canvas></canvas></body></html>");
        assert!(out.contains("<canvas></canvas><script>"));
        assert!(out.ends_with("</script></body></html>"));
    }
}

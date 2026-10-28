//! Chat interna: client del server "Postamon interno" (cartella `server/`).
//!
//! - Il server ha un certificato autofirmato: al primo accesso Postamon ne
//!   memorizza l'impronta SHA-256 e da lì accetta solo quello.
//! - Il token di accesso sta nella Gestione credenziali di Windows.
//! - Un WebSocket tenuto aperto in sottofondo porta i messaggi appena
//!   arrivano; se cade si ricollega da solo.
//! - Gli allegati si caricano e si scaricano a blocchi: se la connessione
//!   cade si riprende da dove si era arrivati.

use std::collections::{HashMap, HashSet};
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use futures::StreamExt;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

/// Come per le caselle: l'accesso alla chat di Postamon Dev resta separato.
const KEYRING_SERVICE: &str = if cfg!(debug_assertions) { "Postamon Dev-interno" } else { "Postamon-interno" };
const DEFAULT_PORT: u16 = 8790;
/// Server proposto quando si attiva la chat, e da cui si scaricano gli
/// aggiornamenti finché nessuna casella è collegata. Lo fissa build.rs alla
/// compilazione; vuoto se non configurato.
pub const DEFAULT_SERVER: &str = env!("POSTAMON_SERVER_CHAT");

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Me {
    pub id: i64,
    pub username: String,
    pub display_name: String,
}

/// Collegamento di una casella, salvato in `interno.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    /// `host:porta`
    pub server: String,
    /// Impronta del certificato del server, fissata al primo accesso.
    pub fingerprint: String,
    pub user: Me,
}

/// Una sessione della chat per ogni casella (ogni casella è uno spazio di
/// lavoro a sé, e l'account della chat è il suo indirizzo).
pub struct Interno {
    path: PathBuf,
    sessions: Mutex<HashMap<String, Saved>>,
    realtime: Mutex<HashMap<String, tauri::async_runtime::JoinHandle<()>>>,
    names: Mutex<HashMap<String, HashMap<i64, String>>>,
    connected: Mutex<HashSet<String>>,
}

fn keyring_entry(account: &str, server: &str) -> Result<keyring::Entry> {
    Ok(keyring::Entry::new(KEYRING_SERVICE, &format!("{account}@{server}"))?)
}

impl Interno {
    pub fn load(path: PathBuf) -> Self {
        let bytes = std::fs::read(&path).unwrap_or_default();
        // Le versioni precedenti salvavano una sessione sola: la si tiene se
        // appartiene a una casella (account = indirizzo email).
        let sessions = serde_json::from_slice::<HashMap<String, Saved>>(&bytes)
            .or_else(|_| {
                serde_json::from_slice::<Saved>(&bytes).map(|s| {
                    let mut map = HashMap::new();
                    if s.user.username.contains('@') {
                        map.insert(s.user.username.clone(), s);
                    }
                    map
                })
            })
            .unwrap_or_default();
        Self {
            path,
            sessions: Mutex::new(sessions),
            realtime: Mutex::default(),
            names: Mutex::default(),
            connected: Mutex::default(),
        }
    }

    pub fn saved(&self, account: &str) -> Option<Saved> {
        self.sessions.lock().unwrap_or_else(|p| p.into_inner()).get(account).cloned()
    }

    pub fn accounts(&self) -> Vec<String> {
        self.sessions.lock().unwrap_or_else(|p| p.into_inner()).keys().cloned().collect()
    }

    /// Server da cui scaricare gli aggiornamenti di Postamon: quello della
    /// chat, con l'impronta del suo certificato, o quello predefinito.
    pub fn update_source(&self) -> (String, Option<String>) {
        let sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        match sessions.values().next() {
            Some(s) => (s.server.clone(), Some(s.fingerprint.clone())),
            None => (DEFAULT_SERVER.to_string(), None),
        }
    }

    pub fn connected(&self, account: &str) -> bool {
        self.connected.lock().unwrap_or_else(|p| p.into_inner()).contains(account)
    }

    fn set_connected(&self, account: &str, on: bool) {
        let mut set = self.connected.lock().unwrap_or_else(|p| p.into_inner());
        if on {
            set.insert(account.to_string());
        } else {
            set.remove(account);
        }
    }

    fn persist(&self) -> Result<()> {
        let sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner()).clone();
        std::fs::write(&self.path, serde_json::to_vec_pretty(&sessions)?)?;
        Ok(())
    }

    // ---- accesso ----

    /// Accesso con indirizzo email e password della casella `account` (li
    /// verifica il server con il server di posta); `display_name` è il nome
    /// per i colleghi.
    pub async fn login(
        &self,
        account: &str,
        server: &str,
        username: &str,
        password: &str,
        display_name: &str,
    ) -> Result<Saved> {
        let server = normalize_server(server)?;
        // Server già visto (da questa o da un'altra casella): il certificato
        // deve essere quello fissato allora.
        let pinned = self
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
            .find(|s| s.server == server)
            .map(|s| s.fingerprint.clone());
        let (client, seen) = http_client(pinned)?;
        let res = client
            .post(format!("https://{server}/api/login"))
            .json(&json!({ "username": username, "password": password, "displayName": display_name }))
            .send()
            .await
            .map_err(|e| connection_error(&server, e))?;
        let body = read_json(res).await?;
        let token = body["token"].as_str().context("risposta del server senza token")?.to_string();
        let user: Me = serde_json::from_value(body["user"].clone())?;
        let fingerprint = seen.lock().unwrap_or_else(|p| p.into_inner()).clone().context("certificato non letto")?;
        let saved = Saved { server, fingerprint, user };
        keyring_entry(account, &saved.server)?
            .set_password(&token)
            .context("salvataggio dell'accesso nella Gestione credenziali")?;
        self.sessions.lock().unwrap_or_else(|p| p.into_inner()).insert(account.to_string(), saved.clone());
        self.persist()?;
        Ok(saved)
    }

    pub async fn logout(&self, account: &str) {
        let _ = self.call(account, reqwest::Method::POST, "/api/logout", Some(json!({}))).await;
        self.stop_realtime(account);
        let removed = self.sessions.lock().unwrap_or_else(|p| p.into_inner()).remove(account);
        if let Some(saved) = removed
            && let Ok(entry) = keyring_entry(account, &saved.server)
        {
            let _ = entry.delete_credential();
        }
        let _ = self.persist();
    }

    /// Client HTTP, indirizzo base e token della sessione di una casella.
    fn session(&self, account: &str) -> Result<(reqwest::Client, String, String)> {
        let saved = self.saved(account).context("Chat interna non attiva per questa casella")?;
        let token = keyring_entry(account, &saved.server)?
            .get_password()
            .map_err(|_| anyhow!("Accesso alla chat interna scaduto: entra di nuovo"))?;
        let (client, _) = http_client(Some(saved.fingerprint))?;
        Ok((client, format!("https://{}", saved.server), token))
    }

    pub async fn call(&self, account: &str, method: reqwest::Method, path: &str, body: Option<Value>) -> Result<Value> {
        let (client, base, token) = self.session(account)?;
        let mut req = client.request(method, format!("{base}{path}")).bearer_auth(token);
        if let Some(body) = body {
            req = req.json(&body);
        }
        let res = req.send().await.map_err(|e| connection_error(&base, e))?;
        read_json(res).await
    }

    // ---- allegati ----

    /// Carica un file a blocchi e restituisce l'id da allegare al messaggio.
    pub async fn upload(
        &self,
        account: &str,
        path: &Path,
        progress: &(dyn Fn(u64, u64) + Send + Sync),
    ) -> Result<String> {
        let size = tokio::fs::metadata(path).await.with_context(|| format!("{}", path.display()))?.len();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
        let mime = mime_guess::from_path(path).first_or_octet_stream().essence_str().to_string();
        let created = self
            .call(
                account,
                reqwest::Method::POST,
                "/api/uploads",
                Some(json!({ "name": name, "mime": mime, "size": size })),
            )
            .await?;
        let id = created["id"].as_str().context("caricamento senza id")?.to_string();
        let chunk = created["chunkSize"].as_u64().unwrap_or(4 * 1024 * 1024);
        let (client, base, token) = self.session(account)?;
        let mut file = tokio::fs::File::open(path).await?;
        let mut offset = 0u64;
        let mut failures = 0u32;
        progress(0, size);
        while offset < size {
            let len = chunk.min(size - offset) as usize;
            let mut buf = vec![0u8; len];
            file.seek(SeekFrom::Start(offset)).await?;
            file.read_exact(&mut buf).await?;
            let sent = client
                .put(format!("{base}/api/uploads/{id}?offset={offset}"))
                .bearer_auth(&token)
                .body(buf)
                .send()
                .await;
            match sent {
                Ok(res) if res.status().is_success() || res.status() == reqwest::StatusCode::CONFLICT => {
                    let v: Value = res.json().await?;
                    offset = v["received"].as_u64().context("risposta senza 'received'")?;
                    failures = 0;
                    progress(offset, size);
                }
                Ok(res) => return Err(read_json(res).await.err().unwrap_or_else(|| anyhow!("caricamento rifiutato"))),
                Err(e) => {
                    failures += 1;
                    if failures > 6 {
                        bail!("Caricamento di {name} interrotto: {e}");
                    }
                    tokio::time::sleep(Duration::from_secs(1 << failures.min(5))).await;
                    if let Ok(v) = self.call(account, reqwest::Method::GET, &format!("/api/uploads/{id}"), None).await {
                        offset = v["received"].as_u64().unwrap_or(offset);
                    }
                }
            }
        }
        Ok(id)
    }

    /// Scarica un file in `dest` passando da `dest.part`, riprendendo da
    /// dove si era arrivati se la connessione cade.
    pub async fn download(
        &self,
        account: &str,
        file_id: &str,
        dest: &Path,
        progress: &(dyn Fn(u64, u64) + Send + Sync),
    ) -> Result<()> {
        let (client, base, token) = self.session(account)?;
        let part = dest.with_extension(format!(
            "{}part",
            dest.extension().map(|e| format!("{}.", e.to_string_lossy())).unwrap_or_default()
        ));
        let mut failures = 0u32;
        loop {
            let have = tokio::fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);
            let mut req = client.get(format!("{base}/api/files/{file_id}")).bearer_auth(&token);
            if have > 0 {
                req = req.header("Range", format!("bytes={have}-"));
            }
            let attempt: Result<()> = async {
                let res = req.send().await?;
                if res.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
                    return Ok(()); // già tutto scaricato
                }
                if !res.status().is_success() {
                    return Err(read_json(res).await.err().unwrap_or_else(|| anyhow!("download rifiutato")));
                }
                let resumed = res.status() == reqwest::StatusCode::PARTIAL_CONTENT;
                let total = if resumed { have } else { 0 } + res.content_length().unwrap_or(0);
                let mut out = if resumed {
                    tokio::fs::OpenOptions::new().append(true).open(&part).await?
                } else {
                    tokio::fs::File::create(&part).await?
                };
                let mut done = if resumed { have } else { 0 };
                let mut stream = res.bytes_stream();
                while let Some(chunk) = stream.next().await {
                    let chunk = chunk?;
                    out.write_all(&chunk).await?;
                    done += chunk.len() as u64;
                    progress(done, total);
                }
                out.flush().await?;
                Ok(())
            }
            .await;
            match attempt {
                Ok(()) => break,
                Err(e) => {
                    failures += 1;
                    if failures > 6 || e.to_string().contains("non trovato") {
                        return Err(e);
                    }
                    tokio::time::sleep(Duration::from_secs(1 << failures.min(5))).await;
                }
            }
        }
        tokio::fs::rename(&part, dest).await?;
        Ok(())
    }

    // ---- tempo reale ----

    /// Avvia il collegamento istantaneo di tutte le caselle con la chat attiva.
    pub fn start_all(self: &Arc<Self>, app: &AppHandle) {
        for account in self.accounts() {
            self.start_realtime(&account, app.clone());
        }
    }

    pub fn start_realtime(self: &Arc<Self>, account: &str, app: AppHandle) {
        self.stop_realtime(account);
        let me = self.clone();
        let acc = account.to_string();
        let handle = tauri::async_runtime::spawn(async move {
            let mut delay = 2u64;
            while me.saved(&acc).is_some() {
                if let Err(e) = me.run_realtime(&acc, &app).await {
                    eprintln!("chat interna ({acc}): {e:#}");
                } else {
                    delay = 2;
                }
                me.set_connected(&acc, false);
                let _ = app.emit("interno-status", json!({ "accountId": acc, "connected": false }));
                tokio::time::sleep(Duration::from_secs(delay)).await;
                delay = (delay * 2).min(60);
            }
        });
        self.realtime.lock().unwrap_or_else(|p| p.into_inner()).insert(account.to_string(), handle);
    }

    pub fn stop_realtime(&self, account: &str) {
        if let Some(h) = self.realtime.lock().unwrap_or_else(|p| p.into_inner()).remove(account) {
            h.abort();
        }
        self.set_connected(account, false);
    }

    async fn run_realtime(&self, account: &str, app: &AppHandle) -> Result<()> {
        let saved = self.saved(account).context("non configurata")?;
        let (_, _, token) = self.session(account)?;
        let (config, _) = tls_config(Some(saved.fingerprint.clone()))?;
        let url = format!("wss://{}/api/ws?token={token}", saved.server);
        let (mut ws, _) = tokio_tungstenite::connect_async_tls_with_config(
            url,
            None,
            false,
            Some(tokio_tungstenite::Connector::Rustls(config)),
        )
        .await?;
        self.set_connected(account, true);
        let _ = app.emit("interno-status", json!({ "accountId": account, "connected": true }));
        self.refresh_names(account).await;
        while let Some(frame) = ws.next().await {
            match frame? {
                tokio_tungstenite::tungstenite::Message::Text(text) => {
                    let Ok(event) = serde_json::from_str::<Value>(&text) else { continue };
                    if event["type"] == "message" {
                        self.notify(app, account, &saved, &event["message"]).await;
                    }
                    let _ = app.emit("interno-event", json!({ "accountId": account, "event": event }));
                }
                tokio_tungstenite::tungstenite::Message::Close(_) => break,
                _ => {}
            }
        }
        Ok(())
    }

    async fn refresh_names(&self, account: &str) {
        if let Ok(Value::Array(users)) = self.call(account, reqwest::Method::GET, "/api/users", None).await {
            let mut all = self.names.lock().unwrap_or_else(|p| p.into_inner());
            let names = all.entry(account.to_string()).or_default();
            for u in users {
                if let (Some(id), Some(name)) = (u["id"].as_i64(), u["displayName"].as_str()) {
                    names.insert(id, name.to_string());
                }
            }
        }
    }

    fn name_of(&self, account: &str, user: i64) -> Option<String> {
        self.names.lock().unwrap_or_else(|p| p.into_inner()).get(account)?.get(&user).cloned()
    }

    /// Notifica di Windows per i messaggi degli altri, quando Postamon non è in primo piano.
    async fn notify(&self, app: &AppHandle, account: &str, saved: &Saved, message: &Value) {
        let sender = message["senderId"].as_i64().unwrap_or(0);
        if sender == saved.user.id {
            return;
        }
        let focused = app.get_webview_window("main").and_then(|w| w.is_focused().ok()).unwrap_or(false);
        if focused {
            return;
        }
        if self.name_of(account, sender).is_none() {
            self.refresh_names(account).await;
        }
        let mut title = self.name_of(account, sender).unwrap_or_else(|| "Chat interna".into());
        // Con più caselle si dice a quale è arrivato.
        if self.accounts().len() > 1 {
            title = format!("{title} → {}", saved.user.display_name);
        }
        let mut body = message["body"].as_str().unwrap_or("").trim().to_string();
        if let Some(files) = message["files"].as_array().filter(|f| !f.is_empty()) {
            let names: Vec<&str> = files.iter().filter_map(|f| f["name"].as_str()).collect();
            let line = format!("Allegato: {}", names.join(", "));
            body = if body.is_empty() { line } else { format!("{body}\n{line}") };
        }
        let _ = app.notification().builder().title(title).body(body).show();
    }
}

/// "192.0.2.10", "https://192.0.2.10:8790/" → "192.0.2.10:8790"
pub fn normalize_server(input: &str) -> Result<String> {
    let s = input.trim().trim_start_matches("https://").trim_start_matches("http://").trim_end_matches('/');
    if s.is_empty() || s.contains(['/', ' ']) {
        bail!("Indirizzo del server non valido");
    }
    let has_port = s.rsplit_once(':').is_some_and(|(_, p)| p.parse::<u16>().is_ok());
    Ok(if has_port { s.to_string() } else { format!("{s}:{DEFAULT_PORT}") })
}

async fn read_json(res: reqwest::Response) -> Result<Value> {
    let status = res.status();
    let body: Value = res.json().await.unwrap_or(Value::Null);
    if status.is_success() {
        return Ok(body);
    }
    if status == reqwest::StatusCode::UNAUTHORIZED && body["error"].as_str() != Some("Nome utente o password non corretti") {
        bail!("Accesso alla chat interna scaduto: entra di nuovo");
    }
    Err(anyhow!(body["error"].as_str().map(str::to_string).unwrap_or_else(|| format!("Errore del server ({status})"))))
}

fn connection_error(server: &str, e: reqwest::Error) -> anyhow::Error {
    let text = format!("{e:?}");
    if text.contains("certificato del server cambiato") {
        anyhow!("Il certificato del server della chat è cambiato: per sicurezza non mi collego")
    } else {
        anyhow!("Server della chat ({server}) non raggiungibile")
    }
}

// ---- certificato ----

#[derive(Debug)]
struct Pinned {
    expected: Option<String>,
    seen: Arc<Mutex<Option<String>>>,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        let fingerprint: String = Sha256::digest(end_entity.as_ref()).iter().map(|b| format!("{b:02x}")).collect();
        *self.seen.lock().unwrap_or_else(|p| p.into_inner()) = Some(fingerprint.clone());
        match &self.expected {
            Some(expected) if *expected != fingerprint => {
                Err(rustls::Error::General("certificato del server cambiato".into()))
            }
            _ => Ok(ServerCertVerified::assertion()),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

type Seen = Arc<Mutex<Option<String>>>;

/// Configurazione TLS che accetta solo il certificato con l'impronta attesa
/// (o qualunque, al primo accesso, annotandone l'impronta).
pub(crate) fn tls_config(expected: Option<String>) -> Result<(Arc<rustls::ClientConfig>, Seen)> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let seen = Seen::default();
    let verifier = Pinned { expected, seen: seen.clone(), provider: provider.clone() };
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    Ok((Arc::new(config), seen))
}

fn http_client(expected: Option<String>) -> Result<(reqwest::Client, Seen)> {
    let (config, seen) = tls_config(expected)?;
    let client = reqwest::Client::builder()
        .tls_backend_preconfigured(Arc::unwrap_or_clone(config))
        .connect_timeout(Duration::from_secs(10))
        .user_agent(concat!("Postamon/", env!("CARGO_PKG_VERSION")))
        .build()?;
    Ok((client, seen))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prova contro un server vero con due utenti di prova: POSTAMON_INTERNO_SERVER,
    /// POSTAMON_INTERNO_U1 e POSTAMON_INTERNO_P1 (nome e password del primo),
    /// POSTAMON_INTERNO_U2 e POSTAMON_INTERNO_P2 (del secondo).
    #[tokio::test]
    async fn live_server_round_trip() {
        let (Ok(server), Ok(u1), Ok(p1), Ok(u2), Ok(p2)) = (
            std::env::var("POSTAMON_INTERNO_SERVER"),
            std::env::var("POSTAMON_INTERNO_U1"),
            std::env::var("POSTAMON_INTERNO_P1"),
            std::env::var("POSTAMON_INTERNO_U2"),
            std::env::var("POSTAMON_INTERNO_P2"),
        ) else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("postamon-interno-client-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let a = Interno::load(dir.join("a.json"));
        let saved = a.login("p1", &server, &u1, &p1, "").await.unwrap();
        assert_eq!(saved.fingerprint.len(), 64);
        assert!(!saved.user.display_name.is_empty());
        let users = a.call("p1", reqwest::Method::GET, "/api/users", None).await.unwrap();
        let p2_id = users.as_array().unwrap().iter().find(|u| u["username"] == u2.as_str()).unwrap()["id"].as_i64().unwrap();
        let conv = a
            .call("p1", reqwest::Method::POST, "/api/conversations", Some(json!({ "memberIds": [p2_id] })))
            .await
            .unwrap();
        let conv_id = conv["id"].as_i64().unwrap();

        // 5 MB e qualcosa: due blocchi da 4 MB.
        let data: Vec<u8> = (0..5_000_123u32).map(|i| (i % 251) as u8).collect();
        let file = dir.join("prova-allegato.bin");
        std::fs::write(&file, &data).unwrap();
        let steps = std::sync::Mutex::new(Vec::new());
        let file_id = a.upload("p1", &file, &|sent, _| steps.lock().unwrap().push(sent)).await.unwrap();
        assert_eq!(steps.lock().unwrap().last(), Some(&(data.len() as u64)));
        assert!(steps.lock().unwrap().len() >= 3);
        a.call(
            "p1",
            reqwest::Method::POST,
            &format!("/api/conversations/{conv_id}/messages"),
            Some(json!({ "body": "Prova dal test automatico", "fileIds": [file_id] })),
        )
        .await
        .unwrap();

        // Seconda casella nella stessa istanza: due sessioni insieme, ognuna
        // con il suo token.
        a.login("p2", &server, &u2, &p2, "").await.unwrap();
        let mut both = a.accounts();
        both.sort();
        assert_eq!(both, vec!["p1".to_string(), "p2".to_string()]);
        let me1 = a.call("p1", reqwest::Method::GET, "/api/me", None).await.unwrap();
        let me2 = a.call("p2", reqwest::Method::GET, "/api/me", None).await.unwrap();
        assert_eq!((me1["username"].as_str(), me2["username"].as_str()), (Some(u1.as_str()), Some(u2.as_str())));
        let convs = a.call("p2", reqwest::Method::GET, "/api/conversations", None).await.unwrap();
        let mine = convs.as_array().unwrap().iter().find(|c| c["id"] == conv_id).unwrap();
        assert!(mine["unread"].as_i64().unwrap() >= 1);
        assert_eq!(mine["lastMessage"]["body"], "Prova dal test automatico");
        let out = dir.join("scaricato.bin");
        a.download("p2", &file_id, &out, &|_, _| {}).await.unwrap();
        assert!(std::fs::read(&out).unwrap() == data, "il file scaricato è diverso");
        // Le sessioni sopravvivono a una riapertura di Postamon.
        assert_eq!(Interno::load(dir.join("a.json")).accounts().len(), 2);
        a.logout("p1").await;
        assert!(a.call("p2", reqwest::Method::GET, "/api/me", None).await.is_ok());
        a.logout("p2").await;
        assert!(a.accounts().is_empty());

        // Certificato diverso da quello fissato (anche da un'altra casella):
        // niente collegamento.
        let fake = Saved {
            server: normalize_server(&server).unwrap(),
            fingerprint: "00".repeat(32),
            user: Me { id: 0, username: "x@x.it".into(), display_name: "x".into() },
        };
        let sessions: HashMap<String, Saved> = [("altra@x.it".to_string(), fake)].into();
        std::fs::write(dir.join("c.json"), serde_json::to_vec(&sessions).unwrap()).unwrap();
        let c = Interno::load(dir.join("c.json"));
        let err = c.login("p1", &server, &u1, &p1, "").await.unwrap_err().to_string();
        assert!(err.contains("certificato"), "{err}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn migrates_the_single_session_file() {
        let dir = std::env::temp_dir().join(format!("postamon-interno-migra-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let old = |user: &str| Saved {
            server: "h:8790".into(),
            fingerprint: "ab".repeat(32),
            user: Me { id: 3, username: user.into(), display_name: "M".into() },
        };
        std::fs::write(dir.join("v1.json"), serde_json::to_vec(&old("marco@ds.it")).unwrap()).unwrap();
        assert_eq!(Interno::load(dir.join("v1.json")).accounts(), vec!["marco@ds.it".to_string()]);
        std::fs::write(dir.join("v1b.json"), serde_json::to_vec(&old("utente1")).unwrap()).unwrap();
        assert!(Interno::load(dir.join("v1b.json")).accounts().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn normalizes_server_addresses() {
        assert_eq!(normalize_server("192.0.2.10").unwrap(), "192.0.2.10:8790");
        assert_eq!(normalize_server("https://192.0.2.10:9000/").unwrap(), "192.0.2.10:9000");
        assert_eq!(normalize_server(" chat.azienda.lan ").unwrap(), "chat.azienda.lan:8790");
        assert!(normalize_server("http://x/y").is_err());
    }
}

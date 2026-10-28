//! Claude collegato a Postamon.
//!
//! Postamon non fa accedere a Claude e non vede mai l'account: Anthropic non
//! permette alle app di terzi di usare l'abbonamento Claude al posto
//! dell'utente. L'abbonamento lo usano i programmi di Anthropic, ognuno con il
//! suo accesso, e Postamon mette a disposizione i suoi strumenti:
//!
//! - Claude Desktop e Claude Code avviano `postamon.exe --mcp`, il server MCP
//!   di Postamon ([`bridge`]).
//! - Il pannello «Claude» dentro Postamon avvia il Claude Code installato sul
//!   PC, così com'è, con i soli strumenti di Postamon ([`code`]).
//!
//! In tutti e due i casi le chiamate arrivano al Postamon aperto su un server
//! locale (127.0.0.1, porta a caso, token in `claude/mcp.json`) e passano da
//! [`tools`]. Leggere non chiede niente; inviare, cestinare, spostare e
//! installare o togliere add-on li conferma l'utente in Postamon ([`confirm`]),
//! perché il testo delle email arriva a Claude e può contenere istruzioni
//! scritte da chiunque.

pub mod bridge;
pub mod code;
mod confirm;
mod tools;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Manager, UserAttentionType};

use crate::AppState;

pub use confirm::{Answer, Request};

/// Una chiamata di strumento oltre questa dimensione non è credibile.
const MAX_CALL: u64 = 2 * 1024 * 1024;

/// Modelli che si scelgono nel pannello: nomi brevi di Claude Code, che li
/// porta all'ultima versione disponibile per il piano dell'utente.
pub const MODELS: &[&str] = &["haiku", "sonnet", "opus", "fable"];

/// Impostazioni in `claude/settings.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Claude può usare la posta. Spento finché l'utente non lo accende.
    pub enabled: bool,
    /// Postamon si è aggiunto ai server MCP di Claude Code.
    pub code_linked: bool,
    /// Modello del pannello, uno di [`MODELS`].
    pub model: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self { enabled: false, code_linked: false, model: MODELS[0].to_string() }
    }
}

impl Settings {
    /// Il modello scelto, o quello di base se il file ne ha uno sconosciuto.
    pub fn model(&self) -> &str {
        MODELS.iter().find(|m| **m == self.model).copied().unwrap_or(MODELS[0])
    }
}

/// Cosa sta guardando l'utente, per «questo messaggio».
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Context {
    pub account_id: Option<String>,
    pub folder: Option<String>,
    pub uid: Option<u32>,
    /// C'è una finestra di scrittura aperta: non se ne apre un'altra.
    pub compose_open: bool,
}

/// Indirizzo del server locale, letto da `postamon.exe --mcp`.
#[derive(Serialize, Deserialize)]
struct Endpoint {
    port: u16,
    token: String,
}

/// Chiamata di uno strumento, dal ponte MCP al Postamon aperto.
#[derive(Deserialize)]
struct Call {
    name: String,
    #[serde(default)]
    arguments: Value,
    /// Programma che chiama ("Claude Desktop", "Claude Code", "pannello").
    #[serde(default)]
    client: String,
}

pub struct Claude {
    data_dir: PathBuf,
    dir: PathBuf,
    port: u16,
    token: String,
    server: Mutex<Option<tiny_http::Server>>,
    settings: Mutex<Settings>,
    context: Mutex<Context>,
    pub confirms: confirm::Confirms,
    pub chat: code::Chat,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Claude {
    /// Apre il server locale e scrive dove trovarlo. Le chiamate si servono
    /// solo dopo [`Claude::serve`], quando il resto di Postamon è pronto.
    pub fn start(data_dir: &Path) -> Result<Self> {
        let dir = data_dir.join("claude");
        std::fs::create_dir_all(&dir)?;
        let server = tiny_http::Server::http("127.0.0.1:0").map_err(|e| anyhow!("server per Claude: {e}"))?;
        let port = server
            .server_addr()
            .to_ip()
            .map(|a| a.port())
            .ok_or_else(|| anyhow!("server per Claude senza porta"))?;
        // Un token nuovo a ogni avvio: il ponte rilegge il file a ogni chiamata.
        let token = random_token();
        write_atomic(&dir.join("mcp.json"), &serde_json::to_vec(&Endpoint { port, token: token.clone() })?)?;
        let settings = std::fs::read(dir.join("settings.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Ok(Self {
            data_dir: data_dir.to_path_buf(),
            dir,
            port,
            token,
            server: Mutex::new(Some(server)),
            settings: Mutex::new(settings),
            context: Mutex::default(),
            confirms: confirm::Confirms::default(),
            chat: code::Chat::default(),
        })
    }

    pub fn serve(&self, app: AppHandle, state: Arc<AppState>) {
        let Some(server) = lock(&self.server).take() else { return };
        let (token, port) = (self.token.clone(), self.port);
        let spawned = std::thread::Builder::new().name("claude".into()).spawn(move || {
            for request in server.incoming_requests() {
                let (app, state, token) = (app.clone(), state.clone(), token.clone());
                std::thread::spawn(move || handle(&app, &state, &token, port, request));
            }
        });
        if let Err(e) = spawned {
            eprintln!("server per Claude non avviato: {e}");
        }
    }

    pub fn settings(&self) -> Settings {
        lock(&self.settings).clone()
    }

    pub fn update_settings(&self, change: impl FnOnce(&mut Settings)) -> Result<Settings> {
        let mut settings = lock(&self.settings);
        change(&mut settings);
        write_atomic(&self.dir.join("settings.json"), &serde_json::to_vec_pretty(&*settings)?)?;
        Ok(settings.clone())
    }

    pub fn context(&self) -> Context {
        lock(&self.context).clone()
    }

    pub fn set_context(&self, context: Context) {
        *lock(&self.context) = context;
    }

    /// Cartella dei dati di Postamon, da passare a `postamon.exe --mcp`.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

/// Porta Postamon davanti: l'utente sta parlando con Claude in un altro
/// programma e qui c'è qualcosa da vedere o da confermare.
pub fn bring_to_front(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        let _ = window.request_user_attention(Some(UserAttentionType::Informational));
    }
}

fn handle(app: &AppHandle, state: &Arc<AppState>, token: &str, port: u16, mut request: tiny_http::Request) {
    let header = |name: &'static str| {
        request
            .headers()
            .iter()
            .find(|h| h.field.equiv(name))
            .map(|h| h.value.as_str().to_string())
    };
    // Solo il ponte di Postamon: niente pagine web (Origin), niente nomi
    // diversi da 127.0.0.1 (DNS rebinding) e il token giusto.
    let allowed = request.method() == &tiny_http::Method::Post
        && request.url() == "/tool"
        && header("Origin").is_none()
        && header("Host").as_deref() == Some(format!("127.0.0.1:{port}").as_str())
        && header("Authorization").is_some_and(|v| same(v.strip_prefix("Bearer ").unwrap_or(""), token));
    if !allowed {
        let _ = request.respond(tiny_http::Response::empty(403));
        return;
    }
    let mut body = Vec::new();
    if request.as_reader().take(MAX_CALL + 1).read_to_end(&mut body).is_err() || body.len() as u64 > MAX_CALL {
        let _ = request.respond(tiny_http::Response::empty(413));
        return;
    }
    let outcome = match serde_json::from_slice::<Call>(&body) {
        Err(_) => tools::Outcome::error("Richiesta non valida"),
        Ok(_) if !state.claude.settings().enabled => tools::Outcome::error(
            "In Postamon il collegamento con Claude è spento: l'utente lo accende dal pannello Claude di Postamon.",
        ),
        Ok(call) => {
            let client = if call.client.trim().is_empty() { "Claude".to_string() } else { call.client };
            tauri::async_runtime::block_on(tools::call(app, state, &client, &call.name, call.arguments))
        }
    };
    let json = json!({ "text": outcome.text, "isError": outcome.is_error }).to_string();
    let content_type = tiny_http::Header::from_bytes("Content-Type", "application/json; charset=utf-8").expect("header");
    let _ = request.respond(tiny_http::Response::from_string(json).with_header(content_type));
}

/// Confronto che non si ferma al primo carattere diverso.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn random_token() -> String {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).expect("generatore casuale del sistema");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_tokens() {
        assert!(same("abc", "abc"));
        assert!(!same("abc", "abd"));
        assert!(!same("abc", "ab"));
        assert_eq!(random_token().len(), 48);
    }

    #[test]
    fn model_defaults_to_haiku() {
        assert_eq!(Settings::default().model(), "haiku");
        let before: Settings = serde_json::from_str(r#"{"enabled":true}"#).unwrap();
        assert_eq!(before.model(), "haiku");
        let odd = Settings { model: "--dangerously-skip-permissions".into(), ..Settings::default() };
        assert_eq!(odd.model(), "haiku");
        let chosen = Settings { model: "opus".into(), ..Settings::default() };
        assert_eq!(chosen.model(), "opus");
    }
}

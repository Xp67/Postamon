//! API HTTP e canale WebSocket.
//!
//! Tutte le richieste tranne `/api/hello`, `/api/login` e
//! `/api/aggiornamenti/` vogliono `Authorization: Bearer <token>` (o
//! `?token=` per il WebSocket).
//! Gli allegati si caricano a blocchi: `POST /api/uploads` li annuncia,
//! `PUT /api/uploads/{id}?offset=N` aggiunge un blocco, e se la connessione
//! cade si riprende dal valore `received` di `GET /api/uploads/{id}`.

use std::collections::HashMap;
use std::io::SeekFrom;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use axum::body::{Body, Bytes};
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, FromRequestParts, Path, Query, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio_util::io::ReaderStream;

use crate::auth::{self, AuthError, Throttle, normalize};
use crate::config::Config;
use crate::db::{Db, User};
use crate::hub::{Event, Hub};

/// Dimensione dei blocchi suggerita ai client, e massimo accettato.
const CHUNK: usize = 4 * 1024 * 1024;
const CHUNK_MAX: usize = 16 * 1024 * 1024;
const MAX_BODY_CHARS: usize = 20_000;

pub struct AppState {
    pub cfg: Config,
    pub db: Db,
    pub hub: Hub,
    throttle: Throttle,
    upload_locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

pub type Shared = Arc<AppState>;

impl AppState {
    pub fn new(cfg: Config) -> Result<Shared> {
        std::fs::create_dir_all(cfg.data_dir.join("file"))?;
        let db = Db::open(&cfg.data_dir.join("interno.sqlite"))?;
        for u in &cfg.local_users {
            db.upsert_user(&normalize(&u.username), &u.display_name, "", "local")?;
        }
        Ok(Arc::new(Self {
            cfg,
            db,
            hub: Hub::default(),
            throttle: Throttle::default(),
            upload_locks: Mutex::default(),
        }))
    }

    fn file_path(&self, id: &str) -> PathBuf {
        self.cfg.data_dir.join("file").join(id)
    }

    fn upload_lock(&self, id: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.upload_locks.lock().unwrap_or_else(|p| p.into_inner()).entry(id.to_string()).or_default().clone()
    }
}

pub fn router(state: Shared) -> Router {
    Router::new()
        .route("/api/hello", get(hello))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/me", get(me))
        .route("/api/users", get(users))
        .route("/api/conversations", get(conversations).post(create_conversation))
        .route("/api/conversations/{id}/messages", get(messages).post(send_message))
        .route("/api/conversations/{id}/read", post(mark_read))
        .route("/api/uploads", post(create_upload))
        .route(
            "/api/uploads/{id}",
            get(upload_status).put(upload_chunk).layer(DefaultBodyLimit::max(CHUNK_MAX)),
        )
        .route("/api/files/{id}", get(download))
        .route("/api/ws", get(ws))
        .route("/api/aggiornamenti/{name}", get(update_file))
        .with_state(state)
}

// ---- errori ----

pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        tracing::error!("{e:#}");
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, "Errore del server".into())
    }
}

impl From<std::io::Error> for ApiError {
    fn from(e: std::io::Error) -> Self {
        anyhow::Error::from(e).into()
    }
}

fn err(status: StatusCode, msg: impl Into<String>) -> ApiError {
    ApiError(status, msg.into())
}

type ApiResult<T> = Result<T, ApiError>;

// ---- autenticazione ----

pub struct Auth {
    user: i64,
    token: String,
}

impl FromRequestParts<Shared> for Auth {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &Shared) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::to_string)
            .or_else(|| {
                parts.uri.query()?.split('&').find_map(|p| p.strip_prefix("token=")).map(str::to_string)
            })
            .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "Accesso richiesto"))?;
        let user = state
            .db
            .session_user(&token)?
            .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "Accesso scaduto: entra di nuovo"))?;
        Ok(Auth { user, token })
    }
}

async fn hello() -> Json<serde_json::Value> {
    Json(json!({ "name": "postamon-interno", "version": env!("CARGO_PKG_VERSION") }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoginRequest {
    username: String,
    password: String,
    /// Nome da mostrare ai colleghi (per l'accesso con la casella di posta).
    #[serde(default)]
    display_name: String,
}

#[derive(Serialize)]
struct LoginResponse {
    token: String,
    user: User,
}

async fn login(State(state): State<Shared>, Json(req): Json<LoginRequest>) -> ApiResult<Json<LoginResponse>> {
    let key = normalize(&req.username);
    if state.throttle.locked(&key) {
        return Err(err(StatusCode::TOO_MANY_REQUESTS, "Troppi tentativi sbagliati: riprova fra un minuto"));
    }
    match auth::login(&state.cfg, &req.username, &req.password, &req.display_name).await {
        Ok(login) => {
            state.throttle.succeeded(&key);
            for p in login.directory.iter().filter(|p| !p.username.is_empty()) {
                state.db.upsert_user(&p.username, &p.display_name, &p.email, "ad")?;
            }
            let p = &login.person;
            let id = state.db.upsert_user(&p.username, &p.display_name, &p.email, login.source)?;
            let token = state.db.create_session(id)?;
            let user = state.db.user(id)?.ok_or_else(|| err(StatusCode::INTERNAL_SERVER_ERROR, "utente"))?;
            tracing::info!("accesso di {}", user.username);
            Ok(Json(LoginResponse { token, user }))
        }
        Err(AuthError::Invalid) => {
            state.throttle.failed(&key);
            Err(err(StatusCode::UNAUTHORIZED, "Nome utente o password non corretti"))
        }
        Err(AuthError::Unavailable(e)) => {
            tracing::warn!("verifica dell'accesso non riuscita: {e}");
            Err(err(StatusCode::SERVICE_UNAVAILABLE, "Il server che verifica la password non risponde: riprova fra poco"))
        }
    }
}

async fn logout(State(state): State<Shared>, auth: Auth) -> ApiResult<StatusCode> {
    state.db.delete_session(&auth.token)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn me(State(state): State<Shared>, auth: Auth) -> ApiResult<Json<User>> {
    state.db.user(auth.user)?.map(Json).ok_or_else(|| err(StatusCode::NOT_FOUND, "utente"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UserView {
    #[serde(flatten)]
    user: User,
    online: bool,
}

async fn users(State(state): State<Shared>, _auth: Auth) -> ApiResult<Json<Vec<UserView>>> {
    let online = state.hub.online();
    Ok(Json(
        state.db.users()?.into_iter().map(|u| UserView { online: online.contains(&u.id), user: u }).collect(),
    ))
}

// ---- chat ----

async fn conversations(State(state): State<Shared>, auth: Auth) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(serde_json::to_value(state.db.conversations(auth.user)?).map_err(anyhow::Error::from)?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateConversation {
    member_ids: Vec<i64>,
    #[serde(default)]
    title: String,
}

async fn create_conversation(
    State(state): State<Shared>,
    auth: Auth,
    Json(req): Json<CreateConversation>,
) -> ApiResult<Json<serde_json::Value>> {
    let mut members: Vec<i64> = req.member_ids.into_iter().filter(|m| *m != auth.user).collect();
    members.sort_unstable();
    members.dedup();
    if members.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "Scegli almeno un collega"));
    }
    for m in &members {
        if state.db.user(*m)?.is_none() {
            return Err(err(StatusCode::BAD_REQUEST, "Collega non trovato"));
        }
    }
    let title = req.title.trim().chars().take(120).collect::<String>();
    let is_group = members.len() > 1 || !title.is_empty();
    let id = match (is_group, state.db.find_direct(auth.user, members[0])?) {
        (false, Some(existing)) => existing,
        _ => {
            members.push(auth.user);
            let id = state.db.create_conversation(auth.user, &members, &title, is_group)?;
            state.hub.send_to(&members, &Event::Conversation { conversation_id: id });
            id
        }
    };
    let conv = state.db.conversation(id, auth.user)?;
    Ok(Json(serde_json::to_value(conv).map_err(anyhow::Error::from)?))
}

fn require_member(state: &AppState, conversation: i64, user: i64) -> ApiResult<()> {
    if state.db.is_member(conversation, user)? {
        Ok(())
    } else {
        Err(err(StatusCode::FORBIDDEN, "Non fai parte di questa chat"))
    }
}

#[derive(Deserialize)]
struct MessagesQuery {
    before: Option<i64>,
    limit: Option<u32>,
}

async fn messages(
    State(state): State<Shared>,
    auth: Auth,
    Path(id): Path<i64>,
    Query(q): Query<MessagesQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    require_member(&state, id, auth.user)?;
    let list = state.db.messages(id, q.before, q.limit.unwrap_or(50).clamp(1, 200))?;
    Ok(Json(serde_json::to_value(list).map_err(anyhow::Error::from)?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendMessage {
    #[serde(default)]
    body: String,
    #[serde(default)]
    file_ids: Vec<String>,
}

async fn send_message(
    State(state): State<Shared>,
    auth: Auth,
    Path(id): Path<i64>,
    Json(req): Json<SendMessage>,
) -> ApiResult<Json<serde_json::Value>> {
    require_member(&state, id, auth.user)?;
    let body = req.body.trim_end().to_string();
    if body.trim().is_empty() && req.file_ids.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "Messaggio vuoto"));
    }
    if body.chars().count() > MAX_BODY_CHARS {
        return Err(err(StatusCode::BAD_REQUEST, "Messaggio troppo lungo"));
    }
    let message = state
        .db
        .insert_message(id, auth.user, &body, &req.file_ids)
        .map_err(|e| err(StatusCode::BAD_REQUEST, e.to_string()))?;
    let members = state.db.members(id)?;
    state.hub.send_to(&members, &Event::Message { message: message.clone() });
    Ok(Json(serde_json::to_value(message).map_err(anyhow::Error::from)?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReadRequest {
    message_id: i64,
}

async fn mark_read(
    State(state): State<Shared>,
    auth: Auth,
    Path(id): Path<i64>,
    Json(req): Json<ReadRequest>,
) -> ApiResult<StatusCode> {
    require_member(&state, id, auth.user)?;
    state.db.mark_read(id, auth.user, req.message_id)?;
    let members = state.db.members(id)?;
    state.hub.send_to(
        &members,
        &Event::Read { conversation_id: id, user_id: auth.user, message_id: req.message_id },
    );
    Ok(StatusCode::NO_CONTENT)
}

// ---- allegati ----

#[derive(Deserialize)]
struct CreateUpload {
    name: String,
    #[serde(default)]
    mime: String,
    size: u64,
}

async fn create_upload(
    State(state): State<Shared>,
    auth: Auth,
    Json(req): Json<CreateUpload>,
) -> ApiResult<Json<serde_json::Value>> {
    if req.size > state.cfg.max_file_bytes() {
        return Err(err(
            StatusCode::PAYLOAD_TOO_LARGE,
            format!("Il file supera il limite di {} MB", state.cfg.max_file_mb),
        ));
    }
    let name: String = req.name.rsplit(['/', '\\']).next().unwrap_or("file").trim().chars().take(200).collect();
    let name = if name.is_empty() { "file".to_string() } else { name };
    let mime = if req.mime.trim().is_empty() { "application/octet-stream".to_string() } else { req.mime };
    let id = state.db.create_file(auth.user, &name, &mime, req.size)?;
    tokio::fs::File::create(state.file_path(&id)).await?;
    Ok(Json(json!({ "id": id, "chunkSize": CHUNK, "received": 0 })))
}

async fn upload_status(
    State(state): State<Shared>,
    auth: Auth,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let file = state
        .db
        .file(&id)?
        .filter(|f| f.uploader_id == auth.user)
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "Caricamento non trovato"))?;
    Ok(Json(json!({ "received": file.received, "size": file.size })))
}

#[derive(Deserialize)]
struct OffsetQuery {
    offset: u64,
}

async fn upload_chunk(
    State(state): State<Shared>,
    auth: Auth,
    Path(id): Path<String>,
    Query(q): Query<OffsetQuery>,
    body: Bytes,
) -> ApiResult<Response> {
    let lock = state.upload_lock(&id);
    let _guard = lock.lock().await;
    let file = state
        .db
        .file(&id)?
        .filter(|f| f.uploader_id == auth.user && f.message_id.is_none())
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "Caricamento non trovato"))?;
    if q.offset != file.received {
        // Il client riparte da qui.
        return Ok((StatusCode::CONFLICT, Json(json!({ "received": file.received }))).into_response());
    }
    let end = file.received + body.len() as u64;
    if end > file.size {
        return Err(err(StatusCode::BAD_REQUEST, "Il blocco va oltre la dimensione annunciata"));
    }
    let mut f = tokio::fs::OpenOptions::new().write(true).open(state.file_path(&id)).await?;
    f.seek(SeekFrom::Start(file.received)).await?;
    f.write_all(&body).await?;
    f.set_len(end).await?;
    f.flush().await?;
    state.db.set_received(&id, end)?;
    Ok(Json(json!({ "received": end })).into_response())
}

async fn download(
    State(state): State<Shared>,
    auth: Auth,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let file = state
        .db
        .file_for(&id, auth.user)?
        .filter(|f| f.received == f.size)
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "File non trovato"))?;
    let size = file.size;
    let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok()).and_then(|v| parse_range(v, size));
    let (start, len, partial) = match range {
        Some(Ok((start, end))) => (start, end - start + 1, true),
        Some(Err(())) => {
            return Ok(Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{size}"))
                .body(Body::empty())
                .expect("risposta"));
        }
        None => (0, size, false),
    };
    let mut f = tokio::fs::File::open(state.file_path(&file.id)).await?;
    if start > 0 {
        f.seek(SeekFrom::Start(start)).await?;
    }
    let mut builder = Response::builder()
        .status(if partial { StatusCode::PARTIAL_CONTENT } else { StatusCode::OK })
        .header(header::CONTENT_TYPE, &file.mime)
        .header(header::CONTENT_LENGTH, len)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename*=UTF-8''{}", encode(&file.name)));
    if partial {
        builder = builder.header(header::CONTENT_RANGE, format!("bytes {start}-{}/{size}", start + len - 1));
    }
    Ok(builder.body(Body::from_stream(ReaderStream::new(f.take(len)))).expect("risposta"))
}

/// `bytes=N-` o `bytes=N-M` → (inizio, fine inclusa). `Err` se fuori dal file.
fn parse_range(value: &str, size: u64) -> Option<Result<(u64, u64), ()>> {
    let spec = value.strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None;
    }
    let (a, b) = spec.split_once('-')?;
    let start: u64 = a.trim().parse().ok()?;
    let end = if b.trim().is_empty() { size.saturating_sub(1) } else { b.trim().parse::<u64>().ok()?.min(size.saturating_sub(1)) };
    if size == 0 || start >= size || end < start {
        return Some(Err(()));
    }
    Some(Ok((start, end)))
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }
        })
        .collect()
}

// ---- aggiornamenti di Postamon ----

/// `dati/aggiornamenti/` contiene `latest.json` e gli installer, messi lì da
/// `scripts/pubblica.ps1`. Niente accesso richiesto: dentro la rete sono file
/// pubblici, e Postamon installa solo un pacchetto firmato con la sua chiave.
///
/// In `latest.json` l'indirizzo dell'installer è solo il nome del file: qui
/// diventa un indirizzo completo con il nome con cui il client ha raggiunto il
/// server, così il file non va riscritto se il server cambia indirizzo.
async fn update_file(
    State(state): State<Shared>,
    Path(name): Path<String>,
    uri: axum::http::Uri,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let not_found = || err(StatusCode::NOT_FOUND, "File non trovato");
    if !valid_update_name(&name) {
        return Err(not_found());
    }
    let path = state.cfg.data_dir.join("aggiornamenti").join(&name);
    if name == "latest.json" {
        let Ok(bytes) = tokio::fs::read(&path).await else {
            // Nessuna versione pubblicata: per Postamon "niente di nuovo".
            return Ok(StatusCode::NO_CONTENT.into_response());
        };
        let mut manifest: serde_json::Value = serde_json::from_slice(&bytes).map_err(anyhow::Error::from)?;
        let host = headers
            .get(header::HOST)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
            .or_else(|| uri.authority().map(|a| a.to_string()))
            .unwrap_or_default();
        let scheme = if state.cfg.tls { "https" } else { "http" };
        if let Some(platforms) = manifest["platforms"].as_object_mut() {
            for platform in platforms.values_mut() {
                if let Some(file) = platform["url"].as_str().filter(|u| valid_update_name(u)) {
                    platform["url"] = json!(format!("{scheme}://{host}/api/aggiornamenti/{file}"));
                }
            }
        }
        return Ok(Json(manifest).into_response());
    }
    let file = tokio::fs::File::open(&path).await.map_err(|_| not_found())?;
    let len = file.metadata().await?.len();
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, len)
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\""))
        .body(Body::from_stream(ReaderStream::new(file)))
        .expect("risposta"))
}

/// Solo nomi di file semplici: niente cartelle né file nascosti.
fn valid_update_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

// ---- tempo reale ----

async fn ws(State(state): State<Shared>, auth: Auth, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| client(state, auth.user, socket))
}

async fn client(state: Shared, user: i64, socket: WebSocket) {
    let (mut sink, mut stream) = socket.split();
    let (id, mut rx, first) = state.hub.add(user);
    if first {
        state.hub.broadcast(&Event::Presence { user_id: user, online: true });
    }
    let writer = tokio::spawn(async move {
        let mut ping = tokio::time::interval(Duration::from_secs(25));
        loop {
            tokio::select! {
                msg = rx.recv() => match msg {
                    Some(text) => {
                        if sink.send(WsMessage::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                },
                _ = ping.tick() => {
                    if sink.send(WsMessage::Ping(Bytes::new())).await.is_err() {
                        break;
                    }
                }
            }
        }
    });
    while let Some(Ok(msg)) = stream.next().await {
        if matches!(msg, WsMessage::Close(_)) {
            break;
        }
    }
    writer.abort();
    if state.hub.remove(user, id) {
        state.hub.broadcast(&Event::Presence { user_id: user, online: false });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ranges() {
        assert_eq!(parse_range("bytes=4-", 10), Some(Ok((4, 9))));
        assert_eq!(parse_range("bytes=0-3", 10), Some(Ok((0, 3))));
        assert_eq!(parse_range("bytes=0-99", 10), Some(Ok((0, 9))));
        assert_eq!(parse_range("bytes=10-", 10), Some(Err(())));
        assert_eq!(parse_range("items=1-2", 10), None);
    }

    #[test]
    fn update_names_stay_in_their_folder() {
        assert!(valid_update_name("Postamon_0.1.1_x64-setup.exe"));
        assert!(valid_update_name("latest.json"));
        assert!(!valid_update_name("../interno.sqlite"));
        assert!(!valid_update_name("..\\chiave.pem"));
        assert!(!valid_update_name(".."));
        assert!(!valid_update_name(""));
    }
}

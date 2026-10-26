//! Comandi chiamati dall'interfaccia. Gli errori arrivano all'interfaccia
//! come testo già pronto da mostrare.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_opener::OpenerExt;

use crate::AppState;
use crate::accounts::{self, Account};
use crate::render::{self, Attachment, Rendered, format_person};
use crate::store::{Flag, Folder, Summary};
use crate::{craft, imap, interno, smtp, sync, update};

type Res<T> = std::result::Result<T, String>;
type St<'a> = State<'a, Arc<AppState>>;

fn fail(e: anyhow::Error) -> String {
    format!("{e:#}")
}

/// Limite del server di invio (500 MB) con un margine per la codifica degli allegati.
const MAX_ATTACHMENTS: u64 = 360 * 1024 * 1024;

/// Tipi di file che non si aprono direttamente da un allegato: si salvano.
const DANGEROUS: &[&str] = &[
    "exe", "com", "bat", "cmd", "msi", "msp", "scr", "pif", "cpl", "js", "jse", "vbs", "vbe", "wsf", "wsh", "ps1",
    "psm1", "hta", "jar", "lnk", "reg", "dll", "appx", "msix", "iso", "img", "vhd", "vhdx", "url", "chm", "inf",
    "scf", "application", "gadget",
];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MsgRef {
    pub folder: String,
    pub uid: u32,
}

// ---- caselle ----

#[tauri::command]
pub fn accounts_list(state: St<'_>) -> Vec<Account> {
    state.accounts.all()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewAccount {
    name: String,
    email: String,
    password: String,
    imap_host: String,
    imap_port: u16,
    smtp_host: String,
    smtp_port: u16,
    #[serde(default)]
    accept_invalid_certs: bool,
}

#[tauri::command]
pub async fn account_add(state: St<'_>, input: NewAccount) -> Res<Account> {
    add_account(&state, input).await.map_err(fail)
}

async fn add_account(state: &AppState, input: NewAccount) -> Result<Account> {
    let email = input.email.trim().to_string();
    if !email.contains('@') {
        bail!("Indirizzo email non valido");
    }
    let id = accounts::account_id(&email);
    if state.accounts.contains(&id) {
        bail!("La casella {email} c'è già");
    }
    let acc = Account {
        id,
        email: email.clone(),
        name: input.name.trim().to_string(),
        imap_host: input.imap_host.trim().to_string(),
        imap_port: input.imap_port,
        smtp_host: input.smtp_host.trim().to_string(),
        smtp_port: input.smtp_port,
        signature: accounts::outlook_signature(&email).unwrap_or_default(),
        accept_invalid_certs: input.accept_invalid_certs,
    };
    // L'accesso al server verifica indirizzo e password prima di salvare qualcosa.
    let mut conn = imap::connect(&acc, &input.password).await?;
    let folders = imap::list_folders(&mut conn).await?;
    let _ = conn.session.logout().await;
    accounts::set_password(&acc.email, &input.password)?;
    state.accounts.upsert(acc.clone())?;
    state.store.replace_folders(&acc.id, &folders)?;
    Ok(acc)
}

#[tauri::command]
pub async fn account_remove(state: St<'_>, account_id: String) -> Res<()> {
    let run = async {
        let acc = state.accounts.get(&account_id)?;
        // Con la casella se ne va anche il suo spazio nella chat.
        state.interno.logout(&acc.id).await;
        state.accounts.remove(&acc.id)?;
        state.store.drop_account(&acc.id)?;
        state.imap.forget(&acc.id);
        accounts::delete_password(&acc.email);
        Ok(())
    };
    run.await.map_err(fail)
}

#[tauri::command]
pub fn account_set_signature(state: St<'_>, account_id: String, signature: String) -> Res<Account> {
    let run = || -> Result<Account> {
        let mut acc = state.accounts.get(&account_id)?;
        acc.signature = signature.replace("\r\n", "\n").trim_end().to_string();
        state.accounts.upsert(acc.clone())?;
        Ok(acc)
    };
    run().map_err(fail)
}

// ---- cartelle e sincronizzazione ----

#[tauri::command]
pub fn folders_list(state: St<'_>, account_id: String) -> Res<Vec<Folder>> {
    state.store.folders(&account_id).map_err(fail)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    new_messages: usize,
}

#[tauri::command]
pub async fn sync(
    app: AppHandle,
    state: St<'_>,
    account_id: String,
    folder: String,
    refresh_folders: bool,
) -> Res<SyncReport> {
    run_sync(&app, state.inner(), &account_id, &folder, refresh_folders).await.map_err(fail)
}

async fn run_sync(
    app: &AppHandle,
    state: &Arc<AppState>,
    account_id: &str,
    folder: &str,
    refresh_folders: bool,
) -> Result<SyncReport> {
    let acc = state.accounts.get(account_id)?;
    let outcome = with_imap!(state, acc, |c| async {
        if refresh_folders {
            let list = imap::list_folders(c).await?;
            state.store.replace_folders(&acc.id, &list)?;
        }
        let outcome = imap::sync_folder(c, &state.store, &acc.id, folder).await?;
        if refresh_folders {
            imap::refresh_counts(c, &state.store, &acc.id, folder).await?;
        }
        Ok::<_, anyhow::Error>(outcome)
    })?;
    // Anteprime dei messaggi recenti in sottofondo.
    let (app, state, acc, folder) = (app.clone(), state.clone(), acc.clone(), folder.to_string());
    tauri::async_runtime::spawn(async move {
        if let Ok(n) = sync::prefetch(&state, &acc, &folder).await
            && n > 0
        {
            sync::emit_updated(&app, &acc.id, &folder);
        }
    });
    Ok(SyncReport { new_messages: outcome.new_uids.len() })
}

// ---- messaggi ----

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePage {
    items: Vec<Summary>,
    /// Messaggi in archivio per questa cartella.
    cached: u32,
    /// Messaggi sul server, all'ultima sincronizzazione.
    total: u32,
}

#[tauri::command]
pub fn messages_list(state: St<'_>, account_id: String, folder: String, limit: u32) -> Res<MessagePage> {
    let run = || -> Result<MessagePage> {
        let items = state.store.list(&account_id, &folder, limit, 0)?;
        let cached = state.store.count(&account_id, &folder)?;
        let total = state
            .store
            .folders(&account_id)?
            .into_iter()
            .find(|f| f.name == folder)
            .map(|f| f.total)
            .unwrap_or(0);
        Ok(MessagePage { items, cached, total })
    };
    run().map_err(fail)
}

#[tauri::command]
pub async fn messages_load_older(state: St<'_>, account_id: String, folder: String) -> Res<u32> {
    let run = async {
        let acc = state.accounts.get(&account_id)?;
        with_imap!(state, acc, |c| imap::load_older(c, &state.store, &acc.id, &folder))
    };
    run.await.map_err(fail)
}

async fn load_raw(state: &AppState, acc: &Account, folder: &str, uid: u32) -> Result<Vec<u8>> {
    if let Some(raw) = state.store.body(&acc.id, folder, uid)? {
        return Ok(raw);
    }
    let raw = with_imap!(state, acc, |c| imap::fetch_body(c, folder, uid))?;
    let (preview, has_attachments) = render::digest(&raw);
    state.store.put_body(&acc.id, folder, uid, &raw, &preview, has_attachments)?;
    Ok(raw)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Opened {
    #[serde(flatten)]
    rendered: Rendered,
    /// Il mittente è fra quelli di cui si mostrano sempre le immagini.
    trusted: bool,
}

#[tauri::command]
pub async fn message_open(
    state: St<'_>,
    account_id: String,
    folder: String,
    uid: u32,
    allow_remote: bool,
) -> Res<Opened> {
    open_message(state.inner(), &account_id, &folder, uid, allow_remote).await.map_err(fail)
}

async fn open_message(state: &Arc<AppState>, account_id: &str, folder: &str, uid: u32, allow_remote: bool) -> Result<Opened> {
    let acc = state.accounts.get(account_id)?;
    let raw = load_raw(state, &acc, folder, uid).await?;
    let summary = state.store.summary(&acc.id, folder, uid)?;
    let trusted = match &summary {
        Some(s) if !s.from_addr.is_empty() => state.store.is_trusted(&s.from_addr)?,
        _ => false,
    };
    let rendered = render::render(&raw, allow_remote || trusted)?;
    if summary.is_some_and(|s| !s.seen) {
        // Letto subito in archivio; il server si aggiorna in sottofondo.
        state.store.set_flag(&acc.id, folder, uid, Flag::Seen, true)?;
        state.store.adjust_unseen(&acc.id, folder, -1)?;
        let (state, folder) = (state.clone(), folder.to_string());
        tauri::async_runtime::spawn(async move {
            let _: Result<()> = async { with_imap!(state, acc, |c| imap::set_flag(c, &folder, uid, "\\Seen", true)) }.await;
        });
    }
    Ok(Opened { rendered, trusted })
}

#[tauri::command]
pub async fn message_set_seen(state: St<'_>, account_id: String, folder: String, uid: u32, seen: bool) -> Res<()> {
    let run = async {
        let acc = state.accounts.get(&account_id)?;
        with_imap!(state, acc, |c| imap::set_flag(c, &folder, uid, "\\Seen", seen))?;
        if state.store.set_flag(&acc.id, &folder, uid, Flag::Seen, seen)? {
            state.store.adjust_unseen(&acc.id, &folder, if seen { -1 } else { 1 })?;
        }
        Ok(())
    };
    run.await.map_err(fail)
}

#[tauri::command]
pub async fn message_set_flagged(state: St<'_>, account_id: String, folder: String, uid: u32, flagged: bool) -> Res<()> {
    let run = async {
        let acc = state.accounts.get(&account_id)?;
        with_imap!(state, acc, |c| imap::set_flag(c, &folder, uid, "\\Flagged", flagged))?;
        state.store.set_flag(&acc.id, &folder, uid, Flag::Flagged, flagged)?;
        Ok(())
    };
    run.await.map_err(fail)
}

#[tauri::command]
pub async fn message_delete(state: St<'_>, account_id: String, folder: String, uid: u32) -> Res<()> {
    let run = async {
        let acc = state.accounts.get(&account_id)?;
        let trash = state.store.folder_with_role(&acc.id, "trash")?;
        let unseen = state.store.summary(&acc.id, &folder, uid)?.is_some_and(|s| !s.seen);
        with_imap!(state, acc, |c| imap::delete(c, &folder, uid, trash.as_deref()))?;
        state.store.remove_message(&acc.id, &folder, uid)?;
        if unseen {
            state.store.adjust_unseen(&acc.id, &folder, -1)?;
        }
        Ok(())
    };
    run.await.map_err(fail)
}

// ---- allegati ----

#[tauri::command]
pub async fn attachment_save(
    state: St<'_>,
    account_id: String,
    folder: String,
    uid: u32,
    index: u32,
    path: String,
) -> Res<()> {
    let run = async {
        let acc = state.accounts.get(&account_id)?;
        let raw = load_raw(&state, &acc, &folder, uid).await?;
        let (_, _, data) = render::attachment_data(&raw, index)?;
        std::fs::write(&path, data).with_context(|| format!("Salvataggio in {path} non riuscito"))?;
        Ok(())
    };
    run.await.map_err(fail)
}

#[tauri::command]
pub async fn attachment_open(
    app: AppHandle,
    state: St<'_>,
    account_id: String,
    folder: String,
    uid: u32,
    index: u32,
) -> Res<()> {
    let run = async {
        let acc = state.accounts.get(&account_id)?;
        let raw = load_raw(&state, &acc, &folder, uid).await?;
        let (name, _, data) = render::attachment_data(&raw, index)?;
        let ext = Path::new(&name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if DANGEROUS.contains(&ext.as_str()) {
            bail!("Per sicurezza i file .{ext} non si aprono da qui: salvali e controllali prima di aprirli");
        }
        let path = write_temp(&state, &acc, uid, &name, &data)?;
        app.opener()
            .open_path(path.to_string_lossy(), None::<&str>)
            .context("Nessun programma per aprire questo file")?;
        Ok(())
    };
    run.await.map_err(fail)
}

/// Copia dell'allegato in una cartella temporanea, per aprirlo con un programma.
fn write_temp(state: &AppState, acc: &Account, uid: u32, name: &str, data: &[u8]) -> Result<std::path::PathBuf> {
    let dir = state.temp_dir.join(format!("{}-{uid}", safe_name(&acc.id)));
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(safe_name(name));
    std::fs::write(&path, data).context("Preparazione dell'allegato non riuscita")?;
    Ok(path)
}

// ---- ArtCraft ----

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CraftStatus {
    /// Programmi della suite che mancano sul PC.
    missing: Vec<String>,
    total: usize,
}

#[tauri::command]
pub fn craft_status() -> CraftStatus {
    CraftStatus {
        missing: craft::missing_suite().iter().map(|a| a.name.to_string()).collect(),
        total: craft::SUITE.len(),
    }
}

#[tauri::command]
pub async fn craft_install(app: AppHandle, state: St<'_>) -> Res<craft::SetupReport> {
    state
        .craft
        .install_suite(|step| {
            let _ = app.emit("craft-setup", step);
        })
        .await
        .map_err(fail)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    app_id: String,
    app_name: String,
    url: String,
    token: String,
    /// Origine del server delle anteprime, per riconoscere i suoi messaggi.
    origin: String,
    /// L'app è installata anche sul PC e può aprire il file a finestra piena.
    native: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WebProgress {
    app_name: String,
    received: u64,
    total: u64,
}

/// Prepara l'anteprima di un allegato con l'app ArtCraft adatta. `None` se
/// nessuna app ArtCraft apre quel tipo di file.
#[tauri::command]
pub async fn attachment_preview(
    app: AppHandle,
    state: St<'_>,
    account_id: String,
    folder: String,
    uid: u32,
    index: u32,
) -> Res<Option<Preview>> {
    let run = async {
        let acc = state.accounts.get(&account_id)?;
        let raw = load_raw(&state, &acc, &folder, uid).await?;
        let (name, mime, data) = render::attachment_data(&raw, index)?;
        craft_preview(&app, state.inner(), &name, &mime, data).await
    };
    run.await.map_err(fail)
}

/// Prepara l'anteprima ArtCraft di un file (scaricando l'app web la prima
/// volta). `None` se nessuna app ArtCraft apre quel tipo di file.
async fn craft_preview(
    app: &AppHandle,
    state: &Arc<AppState>,
    name: &str,
    mime: &str,
    data: Vec<u8>,
) -> Result<Option<Preview>> {
    let Some(viewer) = craft::viewer_for(name) else {
        return Ok(None);
    };
    if !state.craft.web_installed(viewer) {
        state
            .craft
            .install_web(viewer, |received, total| {
                let _ = app.emit("craft-progress", WebProgress { app_name: viewer.name.into(), received, total });
            })
            .await?;
    } else if state.craft.web_needs_check(viewer) {
        // Nuova versione in sottofondo: vale dalla prossima apertura.
        let state = state.clone();
        tauri::async_runtime::spawn(async move {
            let _ = state.craft.install_web(viewer, |_, _| {}).await;
        });
    }
    let (url, token) = state.craft.open(viewer, name, mime, data);
    Ok(Some(Preview {
        app_id: viewer.id.into(),
        app_name: viewer.name.into(),
        url,
        token,
        origin: state.craft.origin(),
        native: craft::native_viewer(viewer).is_some(),
    }))
}

#[tauri::command]
pub fn preview_close(state: St<'_>, token: String) {
    state.craft.close(&token);
}

/// Apre l'allegato nell'app ArtCraft installata sul PC, a finestra piena.
#[tauri::command]
pub async fn attachment_open_native(
    state: St<'_>,
    account_id: String,
    folder: String,
    uid: u32,
    index: u32,
) -> Res<()> {
    let run = async {
        let acc = state.accounts.get(&account_id)?;
        let raw = load_raw(&state, &acc, &folder, uid).await?;
        let (name, _, data) = render::attachment_data(&raw, index)?;
        let viewer = craft::viewer_for(&name).context("Nessuna app ArtCraft apre questo tipo di file")?;
        let exe = craft::native_viewer(viewer).with_context(|| format!("{} non è installato sul PC", viewer.name))?;
        let path = write_temp(&state, &acc, uid, &name, &data)?;
        std::process::Command::new(exe)
            .arg(path)
            .spawn()
            .with_context(|| format!("Avvio di {} non riuscito", viewer.name))?;
        Ok(())
    };
    run.await.map_err(fail)
}

fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_control() || r#"\/:*?"<>|"#.contains(c) { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').to_string();
    let cleaned: String = cleaned.chars().take(150).collect();
    if cleaned.is_empty() { "allegato".to_string() } else { cleaned }
}

#[tauri::command]
pub fn sender_trust(state: St<'_>, address: String) -> Res<()> {
    state.store.trust(&address).map_err(fail)
}

#[tauri::command]
pub fn contacts_suggest(state: St<'_>, query: String) -> Res<Vec<String>> {
    let query = query.trim();
    if query.len() < 2 {
        return Ok(Vec::new());
    }
    state
        .store
        .contacts(query, 8)
        .map(|list| list.iter().map(format_person).collect())
        .map_err(fail)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    path: String,
    name: String,
    size: u64,
}

#[tauri::command]
pub fn files_info(paths: Vec<String>) -> Res<Vec<FileInfo>> {
    paths
        .into_iter()
        .map(|path| {
            let meta = std::fs::metadata(&path).map_err(|e| format!("{path}: {e}"))?;
            let name = Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            Ok(FileInfo { path, name, size: meta.len() })
        })
        .collect()
}

// ---- scrittura ----

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Compose {
    account_id: String,
    to: String,
    cc: String,
    subject: String,
    body: String,
    in_reply_to: Option<String>,
    references: Option<String>,
    reply: Option<MsgRef>,
    forward: Option<MsgRef>,
    forward_attachments: Vec<Attachment>,
}

#[tauri::command]
pub async fn compose_from(state: St<'_>, account_id: String, folder: String, uid: u32, mode: String) -> Res<Compose> {
    let run = async {
        let acc = state.accounts.get(&account_id)?;
        let raw = load_raw(&state, &acc, &folder, uid).await?;
        compose_reply(&acc, &raw, MsgRef { folder: folder.clone(), uid }, &mode)
    };
    run.await.map_err(fail)
}

fn compose_reply(acc: &Account, raw: &[u8], source: MsgRef, mode: &str) -> Result<Compose> {
    let o = render::original(raw)?;
    let is_me = |p: &(String, String)| p.1.eq_ignore_ascii_case(&acc.email);
    let signature = if acc.signature.trim().is_empty() { String::new() } else { format!("{}\n\n", acc.signature.trim()) };
    if mode == "forward" {
        let mut header = format!(
            "---------- Messaggio inoltrato ----------\nDa: {}\nData: {}\nOggetto: {}\nA: {}\n",
            format_person(&o.from),
            o.date,
            o.subject,
            o.to.iter().map(format_person).collect::<Vec<_>>().join(", ")
        );
        if !o.cc.is_empty() {
            header.push_str(&format!("Cc: {}\n", o.cc.iter().map(format_person).collect::<Vec<_>>().join(", ")));
        }
        return Ok(Compose {
            account_id: acc.id.clone(),
            to: String::new(),
            cc: String::new(),
            subject: with_prefix("I: ", &o.subject, &["i:", "fw:", "fwd:", "inoltro:"]),
            body: format!("\n\n{signature}{header}\n{}", o.text.trim_end()),
            in_reply_to: None,
            references: None,
            reply: None,
            forward: Some(source),
            forward_attachments: render::attachment_list(raw)?,
        });
    }
    let mut to: Vec<(String, String)> = if o.reply_to.is_empty() { vec![o.from.clone()] } else { o.reply_to.clone() };
    // Risposta a un messaggio mandato da me: si scrive di nuovo ai destinatari originali.
    if to.iter().all(is_me) {
        to = o.to.clone();
    }
    let mut cc: Vec<(String, String)> = Vec::new();
    if mode == "replyAll" {
        for p in o.to.iter().chain(o.cc.iter()) {
            let seen = to.iter().chain(cc.iter()).any(|q| q.1.eq_ignore_ascii_case(&p.1));
            if !is_me(p) && !seen {
                cc.push(p.clone());
            }
        }
    }
    let quoted = o
        .text
        .trim_end()
        .lines()
        .map(|l| if l.is_empty() { ">".to_string() } else { format!("> {l}") })
        .collect::<Vec<_>>()
        .join("\n");
    let intro = if o.date.is_empty() {
        format!("{} ha scritto:", format_person(&o.from))
    } else {
        format!("Il {}, {} ha scritto:", o.date, format_person(&o.from))
    };
    let mut references = o.references.iter().map(|r| format!("<{r}>")).collect::<Vec<_>>();
    if let Some(id) = &o.message_id {
        references.push(format!("<{id}>"));
    }
    Ok(Compose {
        account_id: acc.id.clone(),
        to: to.iter().map(format_person).collect::<Vec<_>>().join(", "),
        cc: cc.iter().map(format_person).collect::<Vec<_>>().join(", "),
        subject: with_prefix("R: ", &o.subject, &["r:", "re:", "ris:"]),
        body: format!("\n\n{signature}{intro}\n{quoted}"),
        in_reply_to: o.message_id.as_ref().map(|id| format!("<{id}>")),
        references: (!references.is_empty()).then(|| references.join(" ")),
        reply: Some(source),
        forward: None,
        forward_attachments: Vec::new(),
    })
}

fn with_prefix(prefix: &str, subject: &str, known: &[&str]) -> String {
    let lower = subject.trim_start().to_lowercase();
    if known.iter().any(|k| lower.starts_with(k)) { subject.to_string() } else { format!("{prefix}{subject}") }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Outgoing {
    account_id: String,
    to: String,
    cc: String,
    bcc: String,
    subject: String,
    body: String,
    attachments: Vec<String>,
    in_reply_to: Option<String>,
    references: Option<String>,
    reply: Option<MsgRef>,
    forward: Option<MsgRef>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendReport {
    saved_to_sent: bool,
    warning: Option<String>,
}

#[tauri::command]
pub async fn message_send(app: AppHandle, state: St<'_>, input: Outgoing) -> Res<SendReport> {
    send_message(&app, &state, input).await.map_err(fail)
}

async fn send_message(app: &AppHandle, state: &AppState, input: Outgoing) -> Result<SendReport> {
    let acc = state.accounts.get(&input.account_id)?;
    let mut files = Vec::new();
    let mut total: u64 = 0;
    for p in &input.attachments {
        let path = Path::new(p);
        let data = std::fs::read(path).with_context(|| format!("Lettura di {p} non riuscita"))?;
        total += data.len() as u64;
        files.push(smtp::OutgoingFile {
            name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "allegato".into()),
            mime: mime_guess::from_path(path).first_or_octet_stream().essence_str().to_string(),
            data,
        });
    }
    if let Some(f) = &input.forward {
        let raw = load_raw(state, &acc, &f.folder, f.uid).await?;
        for (name, mime, data) in render::all_attachments(&raw)? {
            total += data.len() as u64;
            files.push(smtp::OutgoingFile { name, mime, data });
        }
    }
    if total > MAX_ATTACHMENTS {
        bail!("Gli allegati sono troppo grandi per il server di posta (massimo circa 360 MB in tutto)");
    }
    let message = smtp::build(
        &acc,
        smtp::Draft {
            to: &input.to,
            cc: &input.cc,
            bcc: &input.bcc,
            subject: &input.subject,
            body: &input.body,
            files,
            in_reply_to: input.in_reply_to.clone(),
            references: input.references.clone(),
        },
    )?;
    let password = accounts::password(&acc)?;
    let raw = smtp::send(&acc, &password, &message).await?;

    // Inviato. Da qui in poi un errore non deve far credere che l'invio sia fallito.
    let sent = state.store.folder_with_role(&acc.id, "sent")?;
    let after: Result<bool> = async {
        with_imap!(state, acc, |c| async {
            let mut saved = false;
            if let Some(sent) = &sent {
                imap::append(c, sent, &raw, "(\\Seen)").await?;
                saved = true;
            }
            if let Some(r) = &input.reply {
                imap::set_flag(c, &r.folder, r.uid, "\\Answered", true).await?;
            }
            Ok::<_, anyhow::Error>(saved)
        })
    }
    .await;
    if let Some(r) = &input.reply {
        let _ = state.store.set_flag(&acc.id, &r.folder, r.uid, Flag::Answered, true);
    }
    Ok(match after {
        Ok(true) => {
            if let Some(sent) = &sent {
                sync::emit_updated(app, &acc.id, sent);
            }
            SendReport { saved_to_sent: true, warning: None }
        }
        Ok(false) => SendReport {
            saved_to_sent: false,
            warning: Some("Sul server non c'è una cartella Posta inviata: la copia non è stata salvata".into()),
        },
        Err(e) => SendReport {
            saved_to_sent: false,
            warning: Some(format!("Messaggio inviato, ma la copia in Posta inviata non è stata salvata: {e:#}")),
        },
    })
}

// ---- chat interna ----

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InternoStatus {
    configured: bool,
    server: Option<String>,
    user: Option<interno::Me>,
    connected: bool,
    /// Server proposto quando si attiva la chat.
    default_server: String,
}

/// Stato della chat nello spazio di lavoro di una casella.
#[tauri::command]
pub fn interno_status(state: St<'_>, account_id: String) -> InternoStatus {
    let saved = state.interno.saved(&account_id);
    InternoStatus {
        configured: saved.is_some(),
        server: saved.as_ref().map(|s| s.server.clone()),
        user: saved.map(|s| s.user),
        connected: state.interno.connected(&account_id),
        default_server: interno::DEFAULT_SERVER.into(),
    }
}

/// Attiva la chat di una casella: si entra con il suo indirizzo e la sua password.
#[tauri::command]
pub async fn interno_login(app: AppHandle, state: St<'_>, account_id: String, server: String) -> Res<interno::Me> {
    let run = async {
        let acc = state.accounts.get(&account_id)?;
        let password = accounts::password(&acc)?;
        let saved = state.interno.login(&acc.id, &server, &acc.email, &password, &acc.name).await?;
        state.interno.start_realtime(&acc.id, app);
        Ok(saved.user)
    };
    run.await.map_err(fail)
}

#[tauri::command]
pub async fn interno_logout(state: St<'_>, account_id: String) -> Res<()> {
    state.interno.logout(&account_id).await;
    Ok(())
}

/// Chiamata all'API del server: `GET` o `POST` su un percorso `/api/...`.
#[tauri::command]
pub async fn interno_call(
    state: St<'_>,
    account_id: String,
    method: String,
    path: String,
    body: Option<serde_json::Value>,
) -> Res<serde_json::Value> {
    if !path.starts_with("/api/") || path.contains("..") {
        return Err("percorso non valido".into());
    }
    let method = match method.as_str() {
        "GET" => reqwest::Method::GET,
        "POST" => reqwest::Method::POST,
        _ => return Err("metodo non valido".into()),
    };
    state.interno.call(&account_id, method, &path, body).await.map_err(fail)
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TransferProgress {
    account_id: String,
    conversation_id: i64,
    name: String,
    index: usize,
    count: usize,
    sent: u64,
    total: u64,
}

/// Carica gli allegati (a blocchi, con avanzamento) e manda il messaggio.
#[tauri::command]
pub async fn interno_send(
    app: AppHandle,
    state: St<'_>,
    account_id: String,
    conversation_id: i64,
    body: String,
    paths: Vec<String>,
) -> Res<serde_json::Value> {
    let run = async {
        let mut file_ids = Vec::new();
        let count = paths.len();
        for (index, p) in paths.iter().enumerate() {
            let path = Path::new(p);
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let progress = |sent, total| {
                let _ = app.emit(
                    "interno-upload",
                    TransferProgress {
                        account_id: account_id.clone(),
                        conversation_id,
                        name: name.clone(),
                        index,
                        count,
                        sent,
                        total,
                    },
                );
            };
            file_ids.push(state.interno.upload(&account_id, path, &progress).await?);
        }
        state
            .interno
            .call(
                &account_id,
                reqwest::Method::POST,
                &format!("/api/conversations/{conversation_id}/messages"),
                Some(serde_json::json!({ "body": body, "fileIds": file_ids })),
            )
            .await
    };
    run.await.map_err(fail)
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadProgress {
    file_id: String,
    received: u64,
    total: u64,
}

async fn interno_fetch(app: &AppHandle, state: &AppState, account: &str, file_id: &str, dest: &Path) -> Result<()> {
    if file_id.is_empty() || !file_id.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("file non valido");
    }
    let progress = |received, total| {
        let _ = app.emit("interno-download", DownloadProgress { file_id: file_id.to_string(), received, total });
    };
    state.interno.download(account, file_id, dest, &progress).await
}

/// Copia locale di un allegato della chat (scaricata una volta sola).
async fn interno_local_copy(
    app: &AppHandle,
    state: &AppState,
    account: &str,
    file_id: &str,
    name: &str,
) -> Result<std::path::PathBuf> {
    let dir = state.temp_dir.join("interno");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}-{}", &file_id[..file_id.len().min(8)], safe_name(name)));
    if !path.is_file() {
        interno_fetch(app, state, account, file_id, &path).await?;
    }
    Ok(path)
}

#[tauri::command]
pub async fn interno_file_save(
    app: AppHandle,
    state: St<'_>,
    account_id: String,
    file_id: String,
    path: String,
) -> Res<()> {
    interno_fetch(&app, &state, &account_id, &file_id, Path::new(&path)).await.map_err(fail)
}

/// Le anteprime si fanno in memoria: oltre questa dimensione si apre il file.
const PREVIEW_MAX: u64 = 300 * 1024 * 1024;

#[tauri::command]
pub async fn interno_file_preview(
    app: AppHandle,
    state: St<'_>,
    account_id: String,
    file_id: String,
    name: String,
    mime: String,
    size: u64,
) -> Res<Option<Preview>> {
    let run = async {
        if craft::viewer_for(&name).is_none() || size > PREVIEW_MAX {
            return Ok(None);
        }
        let path = interno_local_copy(&app, &state, &account_id, &file_id, &name).await?;
        let data = tokio::fs::read(&path).await?;
        craft_preview(&app, state.inner(), &name, &mime, data).await
    };
    run.await.map_err(fail)
}

/// Apre un allegato della chat con l'app ArtCraft installata (`native`) o
/// con il programma predefinito di Windows.
#[tauri::command]
pub async fn interno_file_open(
    app: AppHandle,
    state: St<'_>,
    account_id: String,
    file_id: String,
    name: String,
    native: bool,
) -> Res<()> {
    let run = async {
        let ext = Path::new(&name).extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if DANGEROUS.contains(&ext.as_str()) {
            bail!("Per sicurezza i file .{ext} non si aprono da qui: salvali e controllali prima di aprirli");
        }
        let path = interno_local_copy(&app, &state, &account_id, &file_id, &name).await?;
        if native {
            let viewer = craft::viewer_for(&name).context("Nessuna app ArtCraft apre questo tipo di file")?;
            let exe = craft::native_viewer(viewer).with_context(|| format!("{} non è installato sul PC", viewer.name))?;
            std::process::Command::new(exe).arg(&path).spawn().with_context(|| format!("Avvio di {} non riuscito", viewer.name))?;
        } else {
            app.opener()
                .open_path(path.to_string_lossy(), None::<&str>)
                .context("Nessun programma per aprire questo file")?;
        }
        Ok(())
    };
    run.await.map_err(fail)
}

// ---- aggiornamenti ----

/// Versione installata e aggiornamento già scaricato, se c'è.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    current: String,
    ready: Option<update::UpdateInfo>,
}

#[tauri::command]
pub fn update_status(app: AppHandle, state: St<'_>) -> UpdateStatus {
    UpdateStatus { current: app.package_info().version.to_string(), ready: state.updates.ready() }
}

/// Chiude Postamon e installa l'aggiornamento; l'installer lo riapre.
#[tauri::command]
pub fn update_install(state: St<'_>) -> Res<()> {
    state.updates.install().map_err(fail)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account() -> Account {
        Account {
            id: "marco@ds.it".into(),
            email: "marco@ds.it".into(),
            name: "Marco".into(),
            imap_host: String::new(),
            imap_port: 993,
            smtp_host: String::new(),
            smtp_port: 465,
            signature: "Marco\nDimensione Salotto".into(),
            accept_invalid_certs: false,
        }
    }

    const ORIGINAL: &str = "From: Anna <anna@x.it>\r\nTo: marco@ds.it, Luca <luca@x.it>\r\nCc: info@ds.it\r\n\
Subject: Re: Preventivo\r\nDate: Wed, 21 Oct 2026 10:42:00 +0200\r\nMessage-ID: <m2@x.it>\r\n\
References: <m1@x.it>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nVa bene.\r\nGrazie\r\n";

    #[test]
    fn reply_all_skips_me_and_keeps_thread() {
        let c = compose_reply(&account(), ORIGINAL.as_bytes(), MsgRef { folder: "INBOX".into(), uid: 4 }, "replyAll")
            .unwrap();
        assert_eq!(c.to, "Anna <anna@x.it>");
        assert_eq!(c.cc, "Luca <luca@x.it>, info@ds.it");
        assert_eq!(c.subject, "Re: Preventivo");
        assert_eq!(c.in_reply_to.as_deref(), Some("<m2@x.it>"));
        assert_eq!(c.references.as_deref(), Some("<m1@x.it> <m2@x.it>"));
        assert!(c.body.starts_with("\n\nMarco\nDimensione Salotto\n\nIl 21 ottobre 2026, 10:42, Anna <anna@x.it> ha scritto:\n> Va bene.\n> Grazie"));
    }

    #[test]
    fn forward_prefixes_subject() {
        let c = compose_reply(&account(), ORIGINAL.as_bytes(), MsgRef { folder: "INBOX".into(), uid: 4 }, "forward")
            .unwrap();
        assert_eq!(c.subject, "I: Re: Preventivo");
        assert!(c.to.is_empty());
        assert!(c.body.contains("---------- Messaggio inoltrato ----------\nDa: Anna <anna@x.it>"));
    }

    #[test]
    fn safe_names() {
        assert_eq!(safe_name("a/b:c?.pdf"), "a_b_c_.pdf");
        assert_eq!(safe_name(" .. "), "allegato");
    }
}

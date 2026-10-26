//! Connessione IMAP e operazioni sul server: cartelle, sincronizzazione delle
//! intestazioni, download dei messaggi, flag, cancellazione, copia in
//! "Posta inviata".
//!
//! Ogni casella ha una connessione sola, riusata e protetta da un mutex
//! (`Pool`): i comandi dell'interfaccia e il controllo periodico si mettono
//! in fila invece di aprirne di nuove.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use async_imap::types::{Capability, Fetch, Flag, NameAttribute};
use base64::Engine;
use futures::TryStreamExt;
use tokio::net::TcpStream;
use tokio::sync::OwnedMutexGuard;

use crate::accounts::{self, Account};
use crate::render;
use crate::store::{FlagRow, FolderInfo, HeaderRow, Store};

pub type Session = async_imap::Session<tokio_native_tls::TlsStream<TcpStream>>;

/// Quanti messaggi si scaricano alla prima apertura di una cartella e a ogni
/// "Carica messaggi precedenti".
pub const WINDOW: u32 = 300;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const HEADER_QUERY: &str = "(UID FLAGS RFC822.SIZE INTERNALDATE RFC822.HEADER)";

pub struct Conn {
    pub session: Session,
    selected: Option<String>,
    caps: HashSet<String>,
    last_used: Instant,
}

impl Conn {
    fn has(&self, cap: &str) -> bool {
        self.caps.contains(cap)
    }
}

pub async fn connect(acc: &Account, password: &str) -> Result<Conn> {
    let tcp = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((acc.imap_host.as_str(), acc.imap_port)))
        .await
        .map_err(|_| anyhow!("Il server {} non risponde", acc.imap_host))?
        .with_context(|| format!("Collegamento a {} non riuscito", acc.imap_host))?;
    let mut builder = native_tls::TlsConnector::builder();
    if acc.accept_invalid_certs {
        builder.danger_accept_invalid_certs(true).danger_accept_invalid_hostnames(true);
    }
    let tls = tokio_native_tls::TlsConnector::from(builder.build()?)
        .connect(&acc.imap_host, tcp)
        .await
        .context("Connessione sicura con il server non riuscita")?;
    let mut client = async_imap::Client::new(tls);
    client
        .read_response()
        .await?
        .ok_or_else(|| anyhow!("Il server ha chiuso la connessione"))?;
    let mut session = client
        .login(&acc.email, password)
        .await
        .map_err(|(e, _)| match e {
            async_imap::error::Error::No(_) | async_imap::error::Error::Bad(_) => {
                anyhow!("Accesso rifiutato: controlla indirizzo e password")
            }
            e => anyhow!("Accesso non riuscito: {e}"),
        })?;
    let caps = session
        .capabilities()
        .await?
        .iter()
        .filter_map(|c| match c {
            Capability::Atom(a) => Some(a.to_uppercase()),
            _ => None,
        })
        .collect();
    Ok(Conn { session, selected: None, caps, last_used: Instant::now() })
}

// ---- connessioni condivise ----

type Slot = Arc<tokio::sync::Mutex<Option<Conn>>>;

#[derive(Default)]
pub struct Pool {
    slots: std::sync::Mutex<HashMap<String, Slot>>,
}

/// Uso esclusivo della connessione di una casella.
pub struct Lease {
    guard: OwnedMutexGuard<Option<Conn>>,
}

impl Lease {
    pub fn conn(&mut self) -> &mut Conn {
        self.guard.as_mut().expect("connessione aperta da lease()")
    }

    /// Scarta la connessione: la prossima operazione ne aprirà una nuova.
    pub fn poison(&mut self) {
        *self.guard = None;
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if let Some(c) = self.guard.as_mut() {
            c.last_used = Instant::now();
        }
    }
}

impl Pool {
    fn slot(&self, id: &str) -> Slot {
        let mut slots = self.slots.lock().unwrap_or_else(|p| p.into_inner());
        slots.entry(id.to_string()).or_default().clone()
    }

    pub async fn lease(&self, acc: &Account) -> Result<Lease> {
        let mut guard = self.slot(&acc.id).lock_owned().await;
        // Una connessione ferma da un po' può essere stata chiusa dal server
        // o dalla rete: la si prova con un NOOP prima di usarla.
        let alive = match guard.as_mut() {
            Some(c) if c.last_used.elapsed() < Duration::from_secs(20) => true,
            Some(c) => matches!(
                tokio::time::timeout(Duration::from_secs(10), c.session.noop()).await,
                Ok(Ok(()))
            ),
            None => false,
        };
        if !alive {
            *guard = None;
            let password = accounts::password(acc)?;
            *guard = Some(connect(acc, &password).await?);
        }
        Ok(Lease { guard })
    }

    pub fn forget(&self, id: &str) {
        self.slots.lock().unwrap_or_else(|p| p.into_inner()).remove(id);
    }
}

// ---- cartelle ----

pub async fn list_folders(c: &mut Conn) -> Result<Vec<FolderInfo>> {
    let names: Vec<async_imap::types::Name> = c.session.list(Some(""), Some("*")).await?.try_collect().await?;
    let mut out: Vec<FolderInfo> = Vec::new();
    for n in &names {
        let name = n.name().to_string();
        let delim = n.delimiter().unwrap_or("").to_string();
        let selectable = !n.attributes().iter().any(|a| matches!(a, NameAttribute::NoSelect));
        let segments = path_segments(&name, &delim);
        let depth = segments.len().saturating_sub(1) as u32;
        let leaf = decode_utf7(segments.last().copied().unwrap_or(&name));
        let role = folder_role(&name, &leaf, depth, n.attributes());
        let display = match role {
            Some("inbox") => "Posta in arrivo".to_string(),
            Some("sent") if depth == 0 => "Posta inviata".to_string(),
            Some("drafts") if depth == 0 => "Bozze".to_string(),
            Some("trash") if depth == 0 => "Cestino".to_string(),
            Some("junk") if depth == 0 => "Posta indesiderata".to_string(),
            Some("archive") if depth == 0 => "Archivio".to_string(),
            _ => leaf,
        };
        out.push(FolderInfo { name, display, role, depth, selectable, position: role_position(role) });
    }
    // Le sottocartelle seguono la cartella principale a cui appartengono.
    let tops: HashMap<String, i64> = out
        .iter()
        .filter(|f| f.depth == 0)
        .map(|f| (top_segment(&f.name, &names), f.position))
        .collect();
    for f in out.iter_mut().filter(|f| f.depth > 0) {
        if let Some(p) = tops.get(&top_segment(&f.name, &names)) {
            f.position = *p;
        }
    }
    Ok(out)
}

/// Segmenti del percorso senza il prefisso "INBOX." che alcuni server
/// (Courier, certi Dovecot) mettono davanti a tutte le cartelle.
fn path_segments<'a>(name: &'a str, delim: &str) -> Vec<&'a str> {
    if delim.is_empty() {
        return vec![name];
    }
    let rest = match name.get(..5) {
        Some(p) if p.eq_ignore_ascii_case("INBOX") && name[5..].starts_with(delim) => &name[5 + delim.len()..],
        _ => name,
    };
    rest.split(delim).collect()
}

fn top_segment(name: &str, names: &[async_imap::types::Name]) -> String {
    let delim = names
        .iter()
        .find(|n| n.name() == name)
        .and_then(|n| n.delimiter())
        .unwrap_or("");
    path_segments(name, delim).first().copied().unwrap_or(name).to_lowercase()
}

fn folder_role(name: &str, leaf: &str, depth: u32, attrs: &[NameAttribute<'_>]) -> Option<&'static str> {
    if name.eq_ignore_ascii_case("INBOX") {
        return Some("inbox");
    }
    for a in attrs {
        let role = match a {
            NameAttribute::Sent => "sent",
            NameAttribute::Trash => "trash",
            NameAttribute::Drafts => "drafts",
            NameAttribute::Junk => "junk",
            NameAttribute::Archive => "archive",
            _ => continue,
        };
        return Some(role);
    }
    if depth > 0 {
        return None;
    }
    match leaf.to_lowercase().as_str() {
        "sent" | "sent items" | "sent messages" | "posta inviata" | "inviata" | "inviati" | "elementi inviati" => {
            Some("sent")
        }
        "trash" | "deleted" | "deleted items" | "deleted messages" | "cestino" | "posta eliminata"
        | "elementi eliminati" => Some("trash"),
        "drafts" | "bozze" => Some("drafts"),
        "spam" | "junk" | "junk e-mail" | "posta indesiderata" => Some("junk"),
        "archive" | "archivio" => Some("archive"),
        _ => None,
    }
}

fn role_position(role: Option<&str>) -> i64 {
    match role {
        Some("inbox") => 0,
        Some("drafts") => 1,
        Some("sent") => 2,
        Some("archive") => 3,
        Some("junk") => 20,
        Some("trash") => 21,
        _ => 10,
    }
}

/// Decodifica i nomi di cartella IMAP (UTF-7 modificato, RFC 3501 §5.1.3):
/// "Attivit&AOA-" → "Attività".
pub fn decode_utf7(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('-') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let encoded = &after[..end];
        if encoded.is_empty() {
            out.push('&');
        } else {
            let decoded = base64::engine::general_purpose::STANDARD_NO_PAD
                .decode(encoded.replace(',', "/"))
                .ok()
                .map(|bytes| {
                    let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect();
                    String::from_utf16_lossy(&units)
                });
            match decoded {
                Some(text) => out.push_str(&text),
                None => out.push_str(&rest[start..start + 2 + end]),
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Contatori di tutte le cartelle tranne `skip` (quella appena sincronizzata).
pub async fn refresh_counts(c: &mut Conn, store: &Store, account: &str, skip: &str) -> Result<bool> {
    let mut changed = false;
    for f in store.folders(account)? {
        if !f.selectable || f.name == skip {
            continue;
        }
        match c.session.status(&f.name, "(MESSAGES UNSEEN)").await {
            Ok(mbox) => changed |= store.set_counts(account, &f.name, mbox.unseen.unwrap_or(0), mbox.exists)?,
            // Una cartella sparita nel frattempo non deve bloccare le altre.
            Err(async_imap::error::Error::No(_)) => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(changed)
}

// ---- sincronizzazione ----

pub struct SyncOutcome {
    /// Messaggi arrivati dall'ultima sincronizzazione (vuoto alla prima).
    pub new_uids: Vec<u32>,
    pub changed: bool,
}

pub async fn sync_folder(c: &mut Conn, store: &Store, account: &str, folder: &str) -> Result<SyncOutcome> {
    let mbox = c.session.select(folder).await?;
    c.selected = Some(folder.to_string());
    let validity = mbox.uid_validity.unwrap_or(0);
    if store.uidvalidity(account, folder)? != Some(validity) {
        store.reset_folder(account, folder, validity)?;
    }
    let max = store.max_uid(account, folder)?;
    let mut new_uids = Vec::new();
    let mut changed = false;
    if mbox.exists == 0 {
        changed |= store.clear_folder(account, folder)?;
    } else {
        let rows: Vec<HeaderRow> = match max {
            None => {
                let start = mbox.exists.saturating_sub(WINDOW) + 1;
                fetch_headers(&mut c.session, &format!("{start}:*"), false).await?
            }
            // "n:*" restituisce sempre almeno l'ultimo messaggio: si tengono
            // solo quelli davvero nuovi.
            Some(max) => fetch_headers(&mut c.session, &format!("{}:*", max + 1), true)
                .await?
                .into_iter()
                .filter(|r| r.uid > max)
                .collect(),
        };
        if !rows.is_empty() {
            changed = true;
            store.insert_headers(account, folder, &rows)?;
            if max.is_some() {
                new_uids = rows.iter().map(|r| r.uid).collect();
            }
        }
        if let (Some(lo), Some(hi)) = (store.min_uid(account, folder)?, max) {
            let flags = fetch_flags(&mut c.session, &format!("{lo}:{hi}")).await?;
            changed |= store.sync_flags(account, folder, lo, hi, &flags)? > 0;
        }
    }
    let unseen = c.session.search("UNSEEN").await?.len() as u32;
    changed |= store.set_counts(account, folder, unseen, mbox.exists)?;
    Ok(SyncOutcome { new_uids, changed })
}

/// Scarica le intestazioni dei messaggi più vecchi di quelli già in archivio.
pub async fn load_older(c: &mut Conn, store: &Store, account: &str, folder: &str) -> Result<u32> {
    let Some(min) = store.min_uid(account, folder)? else {
        return Ok(0);
    };
    if min <= 1 {
        return Ok(0);
    }
    ensure_selected(c, folder).await?;
    let mut uids: Vec<u32> = c
        .session
        .uid_search(format!("UID 1:{}", min - 1))
        .await?
        .into_iter()
        .filter(|u| *u < min)
        .collect();
    uids.sort_unstable();
    let batch: Vec<u32> = uids.iter().rev().take(WINDOW as usize).copied().collect();
    if batch.is_empty() {
        return Ok(0);
    }
    let rows = fetch_headers(&mut c.session, &uid_set(&batch), true).await?;
    store.insert_headers(account, folder, &rows)?;
    Ok(rows.len() as u32)
}

async fn fetch_headers(s: &mut Session, set: &str, by_uid: bool) -> Result<Vec<HeaderRow>> {
    let fetches: Vec<Fetch> = if by_uid {
        s.uid_fetch(set, HEADER_QUERY).await?.try_collect().await?
    } else {
        s.fetch(set, HEADER_QUERY).await?.try_collect().await?
    };
    Ok(fetches.iter().filter_map(header_row).collect())
}

async fn fetch_flags(s: &mut Session, set: &str) -> Result<Vec<FlagRow>> {
    let fetches: Vec<Fetch> = s.uid_fetch(set, "(UID FLAGS)").await?.try_collect().await?;
    Ok(fetches
        .iter()
        .filter_map(|f| {
            let (seen, flagged, answered, deleted) = flags_of(f);
            (!deleted).then_some(FlagRow { uid: f.uid?, seen, flagged, answered })
        })
        .collect())
}

fn flags_of(f: &Fetch) -> (bool, bool, bool, bool) {
    let (mut seen, mut flagged, mut answered, mut deleted) = (false, false, false, false);
    for flag in f.flags() {
        match flag {
            Flag::Seen => seen = true,
            Flag::Flagged => flagged = true,
            Flag::Answered => answered = true,
            Flag::Deleted => deleted = true,
            _ => {}
        }
    }
    (seen, flagged, answered, deleted)
}

fn header_row(f: &Fetch) -> Option<HeaderRow> {
    let uid = f.uid?;
    let (seen, flagged, answered, deleted) = flags_of(f);
    if deleted {
        return None;
    }
    let mut row = render::header_row(f.header().unwrap_or_default());
    row.uid = uid;
    row.seen = seen;
    row.flagged = flagged;
    row.answered = answered;
    row.size = f.size.unwrap_or(0);
    if row.date == 0 {
        row.date = f.internal_date().map(|d| d.timestamp()).unwrap_or(0);
    }
    Some(row)
}

/// "1,2,3,7" → "1:3,7"
fn uid_set(uids: &[u32]) -> String {
    let mut sorted = uids.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut parts = Vec::new();
    let mut i = 0;
    while i < sorted.len() {
        let start = sorted[i];
        let mut end = start;
        while i + 1 < sorted.len() && sorted[i + 1] == end + 1 {
            i += 1;
            end = sorted[i];
        }
        parts.push(if start == end { start.to_string() } else { format!("{start}:{end}") });
        i += 1;
    }
    parts.join(",")
}

async fn ensure_selected(c: &mut Conn, folder: &str) -> Result<()> {
    if c.selected.as_deref() != Some(folder) {
        c.selected = None;
        c.session.select(folder).await?;
        c.selected = Some(folder.to_string());
    }
    Ok(())
}

// ---- messaggi ----

pub async fn fetch_body(c: &mut Conn, folder: &str, uid: u32) -> Result<Vec<u8>> {
    let mut bodies = fetch_bodies(c, folder, &[uid]).await?;
    bodies.pop().map(|(_, raw)| raw).ok_or_else(|| anyhow!("Il messaggio non è più sul server"))
}

pub async fn fetch_bodies(c: &mut Conn, folder: &str, uids: &[u32]) -> Result<Vec<(u32, Vec<u8>)>> {
    if uids.is_empty() {
        return Ok(Vec::new());
    }
    ensure_selected(c, folder).await?;
    let wanted: HashSet<u32> = uids.iter().copied().collect();
    let fetches: Vec<Fetch> = c.session.uid_fetch(uid_set(uids), "(UID BODY.PEEK[])").await?.try_collect().await?;
    Ok(fetches
        .iter()
        .filter_map(|f| {
            let uid = f.uid.filter(|u| wanted.contains(u))?;
            Some((uid, f.body()?.to_vec()))
        })
        .collect())
}

pub async fn set_flag(c: &mut Conn, folder: &str, uid: u32, flag: &str, on: bool) -> Result<()> {
    ensure_selected(c, folder).await?;
    let op = if on { "+FLAGS.SILENT" } else { "-FLAGS.SILENT" };
    let _: Vec<Fetch> = c.session.uid_store(uid.to_string(), format!("{op} ({flag})")).await?.try_collect().await?;
    Ok(())
}

/// Sposta nel cestino; se il messaggio è già nel cestino lo elimina davvero.
pub async fn delete(c: &mut Conn, folder: &str, uid: u32, trash: Option<&str>) -> Result<()> {
    ensure_selected(c, folder).await?;
    match trash {
        Some(trash) if trash != folder => {
            if c.has("MOVE") {
                c.session.uid_mv(uid.to_string(), trash).await?;
            } else {
                c.session.uid_copy(uid.to_string(), trash).await?;
                expunge_one(c, uid).await?;
            }
        }
        _ => expunge_one(c, uid).await?,
    }
    Ok(())
}

async fn expunge_one(c: &mut Conn, uid: u32) -> Result<()> {
    let _: Vec<Fetch> = c.session.uid_store(uid.to_string(), "+FLAGS.SILENT (\\Deleted)").await?.try_collect().await?;
    if c.has("UIDPLUS") {
        let _: Vec<u32> = c.session.uid_expunge(uid.to_string()).await?.try_collect().await?;
    } else {
        let _: Vec<u32> = c.session.expunge().await?.try_collect().await?;
    }
    Ok(())
}

pub async fn append(c: &mut Conn, folder: &str, raw: &[u8], flags: &str) -> Result<()> {
    c.session.append(folder, Some(flags), None, raw).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_modified_utf7() {
        assert_eq!(decode_utf7("Attivit&AOA-"), "Attività");
        assert_eq!(decode_utf7("Soci &- Partner"), "Soci & Partner");
        assert_eq!(decode_utf7("INBOX"), "INBOX");
        assert_eq!(decode_utf7("&ZeVnLIqe-"), "日本語");
    }

    #[test]
    fn strips_inbox_prefix() {
        assert_eq!(path_segments("INBOX.Sent", "."), vec!["Sent"]);
        assert_eq!(path_segments("INBOX.Clienti.Rossi", "."), vec!["Clienti", "Rossi"]);
        assert_eq!(path_segments("INBOX", "."), vec!["INBOX"]);
        assert_eq!(path_segments("Lavoro/2026", "/"), vec!["Lavoro", "2026"]);
    }

    #[test]
    fn guesses_roles_from_names() {
        assert_eq!(folder_role("INBOX.Sent", "Sent", 0, &[]), Some("sent"));
        assert_eq!(folder_role("Posta inviata", "Posta inviata", 0, &[]), Some("sent"));
        assert_eq!(folder_role("x", "Cestino", 0, &[]), Some("trash"));
        assert_eq!(folder_role("Clienti.Sent", "Sent", 1, &[]), None);
        assert_eq!(folder_role("Varie", "Varie", 0, &[NameAttribute::Junk]), Some("junk"));
    }

    #[test]
    fn compresses_uid_sets() {
        assert_eq!(uid_set(&[7, 1, 2, 3, 9, 10]), "1:3,7,9:10");
        assert_eq!(uid_set(&[5]), "5");
    }
}

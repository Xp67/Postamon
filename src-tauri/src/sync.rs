//! Controllo periodico della posta in arrivo di tutte le caselle, download
//! anticipato dei messaggi recenti e notifiche di Windows.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;

use crate::AppState;
use crate::accounts::Account;
use crate::{imap, render};

const INTERVAL: Duration = Duration::from_secs(60);
/// I messaggi più grandi (di solito per gli allegati) si scaricano solo quando si aprono.
const PREFETCH_MAX_SIZE: u32 = 2_000_000;
const PREFETCH_BATCH: u32 = 25;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailUpdated {
    pub account_id: String,
    pub folder: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncError {
    pub account_id: String,
    pub message: String,
}

pub fn spawn(app: AppHandle, state: Arc<AppState>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(2)).await;
        loop {
            for acc in state.accounts.all() {
                check_inbox(&app, &state, &acc).await;
            }
            tokio::time::sleep(INTERVAL).await;
        }
    });
}

async fn check_inbox(app: &AppHandle, state: &AppState, acc: &Account) {
    match check_inbox_inner(app, state, acc).await {
        Ok(()) => {
            let _ = app.emit("sync-ok", MailUpdated { account_id: acc.id.clone(), folder: "INBOX".into() });
        }
        Err(e) => {
            let _ = app.emit("sync-error", SyncError { account_id: acc.id.clone(), message: format!("{e:#}") });
        }
    }
}

async fn check_inbox_inner(app: &AppHandle, state: &AppState, acc: &Account) -> Result<()> {
    let folder = "INBOX";
    let outcome = with_imap!(state, acc, |c| imap::sync_folder(c, &state.store, &acc.id, folder))?;
    if !outcome.new_uids.is_empty() {
        notify(app, state, acc, folder, &outcome.new_uids);
    }
    if outcome.changed {
        emit_updated(app, &acc.id, folder);
    }
    if prefetch(state, acc, folder).await? > 0 {
        emit_updated(app, &acc.id, folder);
    }
    Ok(())
}

pub fn emit_updated(app: &AppHandle, account_id: &str, folder: &str) {
    let _ = app.emit("mail-updated", MailUpdated { account_id: account_id.to_string(), folder: folder.to_string() });
}

/// Scarica i messaggi recenti non ancora in archivio, per avere anteprima e
/// apertura immediata.
pub async fn prefetch(state: &AppState, acc: &Account, folder: &str) -> Result<usize> {
    let uids = state.store.uids_missing_body(&acc.id, folder, PREFETCH_MAX_SIZE, PREFETCH_BATCH)?;
    if uids.is_empty() {
        return Ok(0);
    }
    let bodies = with_imap!(state, acc, |c| imap::fetch_bodies(c, folder, &uids))?;
    for (uid, raw) in &bodies {
        let (preview, has_attachments) = render::digest(raw);
        state.store.put_body(&acc.id, folder, *uid, raw, &preview, has_attachments)?;
    }
    Ok(bodies.len())
}

fn notify(app: &AppHandle, state: &AppState, acc: &Account, folder: &str, uids: &[u32]) {
    let Ok(list) = state.store.summaries(&acc.id, folder, uids) else {
        return;
    };
    let unseen: Vec<_> = list.into_iter().filter(|m| !m.seen).collect();
    let (title, body) = match unseen.as_slice() {
        [] => return,
        [m] => (
            if m.from_name.is_empty() { m.from_addr.clone() } else { m.from_name.clone() },
            if m.subject.is_empty() { "(senza oggetto)".to_string() } else { m.subject.clone() },
        ),
        many => (format!("{} nuovi messaggi", many.len()), acc.email.clone()),
    };
    let _ = app.notification().builder().title(title).body(body).show();
}

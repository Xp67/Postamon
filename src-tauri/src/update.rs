//! Aggiornamenti automatici di Postamon.
//!
//! - Gli installer si scaricano dal server della chat interna
//!   (`/api/aggiornamenti/latest.json`), che ogni PC raggiunge già: quello a
//!   cui è collegata la chat di una casella, altrimenti quello predefinito.
//! - Si controlla poco dopo l'avvio e poi ogni 4 ore; l'installer si scarica
//!   in sottofondo e all'utente resta solo "Riavvia e aggiorna".
//! - Ogni installer è firmato con la chiave di Postamon, versione compresa:
//!   un pacchetto senza la firma giusta non si installa. Il certificato del
//!   server si controlla come per la chat (impronta fissata, se nota).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::{AppState, interno};

const FIRST_CHECK: Duration = Duration::from_secs(30);
const EVERY: Duration = Duration::from_secs(4 * 3600);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub notes: String,
}

struct Ready {
    update: Update,
    bytes: Vec<u8>,
}

/// Aggiornamento già scaricato e verificato, in attesa del riavvio.
#[derive(Default)]
pub struct Updates {
    ready: Mutex<Option<Ready>>,
}

impl Updates {
    pub fn ready(&self) -> Option<UpdateInfo> {
        self.ready.lock().unwrap_or_else(|p| p.into_inner()).as_ref().map(|r| info(&r.update))
    }

    /// Lancia l'installer e chiude Postamon; l'installer lo riapre aggiornato.
    pub fn install(&self) -> Result<()> {
        let ready = self.ready.lock().unwrap_or_else(|p| p.into_inner()).take().context("Nessun aggiornamento pronto")?;
        ready.update.install(&ready.bytes).context("Installazione dell'aggiornamento non riuscita")?;
        Ok(())
    }
}

fn info(update: &Update) -> UpdateInfo {
    UpdateInfo { version: update.version.clone(), notes: update.body.clone().unwrap_or_default() }
}

pub fn spawn(app: AppHandle, state: Arc<AppState>) {
    // Una copia di sviluppo non si aggiorna: l'installer metterebbe il
    // Postamon pubblicato al posto di quello installato.
    if cfg!(debug_assertions) && std::env::var_os("POSTAMON_UPDATE_SERVER").is_none() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            if state.updates.ready().is_none()
                && let Err(e) = check(&app, &state).await
            {
                // Server spento o irraggiungibile: si riprova al giro dopo.
                eprintln!("aggiornamenti: {e:#}");
            }
            tokio::time::sleep(EVERY).await;
        }
    });
}

async fn check(app: &AppHandle, state: &AppState) -> Result<()> {
    // POSTAMON_UPDATE_SERVER serve per le prove con un server di test.
    let (server, fingerprint) = match std::env::var("POSTAMON_UPDATE_SERVER") {
        Ok(s) => (interno::normalize_server(&s)?, None),
        Err(_) => state.interno.update_source(),
    };
    // Nessuna chat attiva e nessun server predefinito: non c'è dove guardare.
    if server.is_empty() {
        return Ok(());
    }
    let (tls, _) = interno::tls_config(fingerprint)?;
    let updater = app
        .updater_builder()
        .endpoints(vec![format!("https://{server}/api/aggiornamenti/latest.json").parse()?])?
        .configure_client(move |client| {
            client.tls_backend_preconfigured((*tls).clone()).connect_timeout(Duration::from_secs(10))
        })
        .build()?;
    let Some(update) = updater.check().await? else {
        return Ok(());
    };
    // `download` verifica la firma prima di restituire i byte.
    let bytes = update.download(|_, _| {}, || {}).await?;
    let ready = info(&update);
    *state.updates.ready.lock().unwrap_or_else(|p| p.into_inner()) = Some(Ready { update, bytes });
    let _ = app.emit("update-ready", &ready);
    Ok(())
}

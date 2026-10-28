//! Azioni di Claude che l'utente conferma in Postamon.
//!
//! Lo strumento che chiede un'azione aspetta la risposta per un po'; se
//! l'utente non risponde in tempo, Claude riceve il numero della richiesta e
//! può chiederne l'esito più tardi. L'azione parte comunque solo quando
//! l'utente la accetta, anche se lo strumento ha già smesso di aspettare.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;

use super::lock;

/// Quanto aspetta lo strumento prima di rispondere a Claude «in attesa»:
/// sotto il minuto, il tempo massimo di molti programmi per una chiamata.
const WAIT: Duration = Duration::from_secs(45);
/// Le richieste a cui nessuno risponde si scartano dopo un'ora.
const EXPIRE: Duration = Duration::from_secs(3600);

pub type Action = Box<dyn FnOnce() -> BoxFuture<'static, Result<String>> + Send>;

/// Richiesta da mostrare all'utente.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub id: u64,
    /// Programma che la chiede ("Claude Desktop", "Claude Code", "pannello").
    pub client: String,
    /// "send", "move", "trash", "addon-install", "addon-uninstall".
    pub kind: String,
    /// Cosa mostrare, diverso per ogni tipo.
    pub detail: Value,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Answer {
    Accept,
    Reject,
    /// Solo per i messaggi: l'utente lo apre nella finestra di scrittura.
    Edit,
}

struct Pending {
    request: Request,
    action: Action,
    created: Instant,
    waiter: Option<tokio::sync::oneshot::Sender<String>>,
}

#[derive(Default)]
pub struct Confirms {
    next: AtomicU64,
    pending: Mutex<Vec<Pending>>,
    done: Mutex<HashMap<u64, String>>,
}

impl Confirms {
    /// Mostra la richiesta e aspetta la risposta (al massimo [`WAIT`]).
    /// Restituisce cosa dire a Claude.
    pub async fn ask(&self, app: &AppHandle, client: &str, kind: &str, detail: Value, action: Action) -> String {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let request = Request { id, client: client.to_string(), kind: kind.to_string(), detail };
        let (tx, rx) = tokio::sync::oneshot::channel();
        {
            let mut pending = lock(&self.pending);
            pending.retain(|p| p.created.elapsed() < EXPIRE);
            pending.push(Pending { request: request.clone(), action, created: Instant::now(), waiter: Some(tx) });
        }
        let _ = app.emit("claude-request", &request);
        let _ = app
            .notification()
            .builder()
            .title("Claude chiede una conferma")
            .body("Apri Postamon per vedere cosa vuole fare")
            .show();
        super::bring_to_front(app);
        match tokio::time::timeout(WAIT, rx).await {
            Ok(Ok(outcome)) => outcome,
            _ => format!(
                "L'utente non ha ancora risposto in Postamon (richiesta n. {id}). Non è stato fatto niente: \
                 chiedi all'utente di rispondere in Postamon, poi controlla con request_status."
            ),
        }
    }

    pub fn list(&self) -> Vec<Request> {
        lock(&self.pending).iter().map(|p| p.request.clone()).collect()
    }

    /// Risposta dell'utente. Con [`Answer::Accept`] esegue l'azione;
    /// l'errore, se c'è, torna anche all'interfaccia.
    pub async fn answer(&self, id: u64, answer: Answer) -> Result<String> {
        let p = {
            let mut pending = lock(&self.pending);
            let Some(i) = pending.iter().position(|p| p.request.id == id) else {
                bail!("Questa richiesta di Claude non c'è più");
            };
            if answer == Answer::Edit && pending[i].request.kind != "send" {
                bail!("Questa richiesta non si modifica");
            }
            pending.remove(i)
        };
        let result = match answer {
            Answer::Accept => (p.action)().await,
            Answer::Reject => Ok("L'utente ha rifiutato: non è stato fatto niente.".to_string()),
            Answer::Edit => Ok("L'utente ha aperto il messaggio nella finestra di scrittura di Postamon per \
                 modificarlo: lo invierà lui, se vuole."
                .to_string()),
        };
        let outcome = match &result {
            Ok(text) => text.clone(),
            Err(e) => format!("Non riuscito: {e:#}"),
        };
        lock(&self.done).insert(id, outcome.clone());
        if let Some(waiter) = p.waiter {
            let _ = waiter.send(outcome);
        }
        result
    }

    /// Esito di una richiesta, per lo strumento `request_status`.
    pub fn status(&self, id: u64) -> String {
        if let Some(outcome) = lock(&self.done).get(&id) {
            return outcome.clone();
        }
        if lock(&self.pending).iter().any(|p| p.request.id == id) {
            return format!("La richiesta n. {id} aspetta ancora la risposta dell'utente in Postamon.");
        }
        format!("Non c'è nessuna richiesta n. {id}: forse è scaduta o Postamon è stato riaperto.")
    }
}

//! Connessioni aperte (WebSocket) per utente: servono a consegnare subito i
//! messaggi e a sapere chi è collegato.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::db::Message;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Event {
    #[serde(rename_all = "camelCase")]
    Message { message: Message },
    /// Chat nuova o cambiata: il client ricarica l'elenco.
    #[serde(rename_all = "camelCase")]
    Conversation { conversation_id: i64 },
    #[serde(rename_all = "camelCase")]
    Read { conversation_id: i64, user_id: i64, message_id: i64 },
    #[serde(rename_all = "camelCase")]
    Presence { user_id: i64, online: bool },
}

/// Connessioni di un utente: (id della connessione, coda dei messaggi in uscita).
type Connections = Vec<(u64, UnboundedSender<String>)>;

#[derive(Default)]
pub struct Hub {
    conns: Mutex<HashMap<i64, Connections>>,
    next: AtomicU64,
}

impl Hub {
    /// Registra una connessione; `true` se è la prima dell'utente.
    pub fn add(&self, user: i64) -> (u64, UnboundedReceiver<String>, bool) {
        let (tx, rx) = unbounded_channel();
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let mut conns = self.conns.lock().unwrap_or_else(|p| p.into_inner());
        let list = conns.entry(user).or_default();
        list.push((id, tx));
        (id, rx, list.len() == 1)
    }

    /// Toglie una connessione; `true` se era l'ultima dell'utente.
    pub fn remove(&self, user: i64, id: u64) -> bool {
        let mut conns = self.conns.lock().unwrap_or_else(|p| p.into_inner());
        let Some(list) = conns.get_mut(&user) else { return false };
        list.retain(|(i, _)| *i != id);
        if list.is_empty() {
            conns.remove(&user);
            true
        } else {
            false
        }
    }

    pub fn send_to(&self, users: &[i64], event: &Event) {
        let Ok(text) = serde_json::to_string(event) else { return };
        let conns = self.conns.lock().unwrap_or_else(|p| p.into_inner());
        for user in users {
            for (_, tx) in conns.get(user).into_iter().flatten() {
                let _ = tx.send(text.clone());
            }
        }
    }

    pub fn broadcast(&self, event: &Event) {
        let users: Vec<i64> = self.conns.lock().unwrap_or_else(|p| p.into_inner()).keys().copied().collect();
        self.send_to(&users, event);
    }

    pub fn online(&self) -> HashSet<i64> {
        self.conns.lock().unwrap_or_else(|p| p.into_inner()).keys().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_first_and_last_connection() {
        let hub = Hub::default();
        let (a, mut rx_a, first) = hub.add(7);
        assert!(first);
        let (b, _rx_b, first) = hub.add(7);
        assert!(!first);
        hub.send_to(&[7], &Event::Presence { user_id: 1, online: true });
        assert!(rx_a.try_recv().unwrap().contains("\"type\":\"presence\""));
        assert!(!hub.remove(7, a));
        assert!(hub.remove(7, b));
        assert!(hub.online().is_empty());
    }
}

//! Archivio SQLite: utenti, sessioni, chat, messaggi, file.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

pub struct Db {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS users (
    id           INTEGER PRIMARY KEY,
    username     TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    email        TEXT NOT NULL DEFAULT '',
    source       TEXT NOT NULL,
    last_login   INTEGER
);
CREATE TABLE IF NOT EXISTS sessions (
    token      TEXT PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id),
    expires_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS conversations (
    id         INTEGER PRIMARY KEY,
    title      TEXT NOT NULL DEFAULT '',
    is_group   INTEGER NOT NULL,
    created_by INTEGER NOT NULL REFERENCES users(id),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS members (
    conversation_id INTEGER NOT NULL REFERENCES conversations(id),
    user_id         INTEGER NOT NULL REFERENCES users(id),
    last_read       INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (conversation_id, user_id)
);
CREATE INDEX IF NOT EXISTS members_by_user ON members (user_id);
CREATE TABLE IF NOT EXISTS messages (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    conversation_id INTEGER NOT NULL REFERENCES conversations(id),
    sender_id       INTEGER NOT NULL REFERENCES users(id),
    body            TEXT NOT NULL,
    created_at      INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS messages_by_conversation ON messages (conversation_id, id);
CREATE TABLE IF NOT EXISTS files (
    id          TEXT PRIMARY KEY,
    uploader_id INTEGER NOT NULL REFERENCES users(id),
    name        TEXT NOT NULL,
    mime        TEXT NOT NULL,
    size        INTEGER NOT NULL,
    received    INTEGER NOT NULL DEFAULT 0,
    message_id  INTEGER REFERENCES messages(id),
    created_at  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS files_by_message ON files (message_id);
";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    pub email: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub id: String,
    pub name: String,
    pub mime: String,
    pub size: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: i64,
    pub conversation_id: i64,
    pub sender_id: i64,
    pub body: String,
    pub created_at: i64,
    pub files: Vec<FileInfo>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: i64,
    pub title: String,
    pub is_group: bool,
    pub members: Vec<i64>,
    pub last_message: Option<Message>,
    pub unread: i64,
    pub updated_at: i64,
}

pub struct FileRow {
    pub id: String,
    pub uploader_id: i64,
    pub name: String,
    pub mime: String,
    pub size: u64,
    pub received: u64,
    pub message_id: Option<i64>,
}

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn random_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("generatore casuale del sistema");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Durata di un accesso: si rinnova a ogni nuovo login.
const SESSION_DAYS: i64 = 90;

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn db(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    // ---- utenti e sessioni ----

    /// Crea o aggiorna un utente; restituisce l'id.
    pub fn upsert_user(&self, username: &str, display_name: &str, email: &str, source: &str) -> Result<i64> {
        let db = self.db();
        db.execute(
            "INSERT INTO users (username, display_name, email, source) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (username) DO UPDATE SET display_name = excluded.display_name, email = excluded.email",
            params![username, display_name, email, source],
        )?;
        Ok(db.query_row("SELECT id FROM users WHERE username = ?1", [username], |r| r.get(0))?)
    }

    pub fn create_session(&self, user_id: i64) -> Result<String> {
        let token = format!("{}{}", random_id(), random_id());
        let db = self.db();
        db.execute("UPDATE users SET last_login = ?2 WHERE id = ?1", params![user_id, now()])?;
        db.execute(
            "INSERT INTO sessions (token, user_id, expires_at) VALUES (?1, ?2, ?3)",
            params![token, user_id, now() + SESSION_DAYS * 86_400],
        )?;
        db.execute("DELETE FROM sessions WHERE expires_at < ?1", [now()])?;
        Ok(token)
    }

    pub fn session_user(&self, token: &str) -> Result<Option<i64>> {
        Ok(self
            .db()
            .query_row(
                "SELECT user_id FROM sessions WHERE token = ?1 AND expires_at > ?2",
                params![token, now()],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn delete_session(&self, token: &str) -> Result<()> {
        self.db().execute("DELETE FROM sessions WHERE token = ?1", [token])?;
        Ok(())
    }

    pub fn user(&self, id: i64) -> Result<Option<User>> {
        Ok(self
            .db()
            .query_row("SELECT id, username, display_name, email FROM users WHERE id = ?1", [id], user_row)
            .optional()?)
    }

    pub fn users(&self) -> Result<Vec<User>> {
        let db = self.db();
        let mut stmt =
            db.prepare("SELECT id, username, display_name, email FROM users ORDER BY display_name COLLATE NOCASE")?;
        let rows = stmt.query_map([], user_row)?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    // ---- chat ----

    /// Chat a due già esistente fra due persone.
    pub fn find_direct(&self, a: i64, b: i64) -> Result<Option<i64>> {
        Ok(self
            .db()
            .query_row(
                "SELECT c.id FROM conversations c
                 JOIN members ma ON ma.conversation_id = c.id AND ma.user_id = ?1
                 JOIN members mb ON mb.conversation_id = c.id AND mb.user_id = ?2
                 WHERE c.is_group = 0 LIMIT 1",
                params![a, b],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn create_conversation(&self, creator: i64, members: &[i64], title: &str, is_group: bool) -> Result<i64> {
        let mut db = self.db();
        let tx = db.transaction()?;
        let t = now();
        tx.execute(
            "INSERT INTO conversations (title, is_group, created_by, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)",
            params![title, is_group, creator, t],
        )?;
        let id = tx.last_insert_rowid();
        for user in members {
            tx.execute(
                "INSERT OR IGNORE INTO members (conversation_id, user_id) VALUES (?1, ?2)",
                params![id, user],
            )?;
        }
        tx.commit()?;
        Ok(id)
    }

    pub fn is_member(&self, conversation: i64, user: i64) -> Result<bool> {
        Ok(self
            .db()
            .query_row(
                "SELECT 1 FROM members WHERE conversation_id = ?1 AND user_id = ?2",
                params![conversation, user],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    pub fn members(&self, conversation: i64) -> Result<Vec<i64>> {
        let db = self.db();
        let mut stmt = db.prepare("SELECT user_id FROM members WHERE conversation_id = ?1 ORDER BY user_id")?;
        let rows = stmt.query_map([conversation], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn conversation(&self, id: i64, for_user: i64) -> Result<Option<Conversation>> {
        Ok(self.conversations(for_user)?.into_iter().find(|c| c.id == id))
    }

    pub fn conversations(&self, user: i64) -> Result<Vec<Conversation>> {
        let rows: Vec<(i64, String, bool, i64, i64)> = {
            let db = self.db();
            let mut stmt = db.prepare(
                "SELECT c.id, c.title, c.is_group, c.updated_at, m.last_read FROM conversations c
                 JOIN members m ON m.conversation_id = c.id AND m.user_id = ?1
                 ORDER BY c.updated_at DESC",
            )?;
            stmt.query_map([user], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
                .collect::<rusqlite::Result<_>>()?
        };
        let mut out = Vec::new();
        for (id, title, is_group, updated_at, last_read) in rows {
            let unread: i64 = self.db().query_row(
                "SELECT COUNT(*) FROM messages WHERE conversation_id = ?1 AND id > ?2 AND sender_id != ?3",
                params![id, last_read, user],
                |r| r.get(0),
            )?;
            out.push(Conversation {
                id,
                title,
                is_group,
                members: self.members(id)?,
                last_message: self.messages(id, None, 1)?.pop(),
                unread,
                updated_at,
            });
        }
        Ok(out)
    }

    pub fn insert_message(&self, conversation: i64, sender: i64, body: &str, file_ids: &[String]) -> Result<Message> {
        let mut db = self.db();
        let tx = db.transaction()?;
        for id in file_ids {
            let ok = tx
                .query_row(
                    "SELECT 1 FROM files WHERE id = ?1 AND uploader_id = ?2 AND received = size AND message_id IS NULL",
                    params![id, sender],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if !ok {
                bail!("allegato non valido o non ancora caricato del tutto");
            }
        }
        let t = now();
        tx.execute(
            "INSERT INTO messages (conversation_id, sender_id, body, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![conversation, sender, body, t],
        )?;
        let id = tx.last_insert_rowid();
        for file in file_ids {
            tx.execute("UPDATE files SET message_id = ?2 WHERE id = ?1", params![file, id])?;
        }
        tx.execute("UPDATE conversations SET updated_at = ?2 WHERE id = ?1", params![conversation, t])?;
        tx.execute(
            "UPDATE members SET last_read = ?3 WHERE conversation_id = ?1 AND user_id = ?2",
            params![conversation, sender, id],
        )?;
        tx.commit()?;
        drop(db);
        Ok(self.messages(conversation, Some(id + 1), 1)?.pop().expect("messaggio appena inserito"))
    }

    /// Messaggi in ordine cronologico, i più recenti prima di `before`.
    pub fn messages(&self, conversation: i64, before: Option<i64>, limit: u32) -> Result<Vec<Message>> {
        let db = self.db();
        let mut stmt = db.prepare(
            "SELECT id, conversation_id, sender_id, body, created_at FROM messages
             WHERE conversation_id = ?1 AND id < ?2 ORDER BY id DESC LIMIT ?3",
        )?;
        let mut rows: Vec<Message> = stmt
            .query_map(params![conversation, before.unwrap_or(i64::MAX), limit], |r| {
                Ok(Message {
                    id: r.get(0)?,
                    conversation_id: r.get(1)?,
                    sender_id: r.get(2)?,
                    body: r.get(3)?,
                    created_at: r.get(4)?,
                    files: Vec::new(),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        rows.reverse();
        if !rows.is_empty() {
            let ids: Vec<String> = rows.iter().map(|m| m.id.to_string()).collect();
            let mut stmt = db.prepare(&format!(
                "SELECT message_id, id, name, mime, size FROM files WHERE message_id IN ({}) ORDER BY rowid",
                ids.join(",")
            ))?;
            let mut files: HashMap<i64, Vec<FileInfo>> = HashMap::new();
            for row in stmt.query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    FileInfo { id: r.get(1)?, name: r.get(2)?, mime: r.get(3)?, size: r.get::<_, i64>(4)? as u64 },
                ))
            })? {
                let (message, file) = row?;
                files.entry(message).or_default().push(file);
            }
            for m in &mut rows {
                m.files = files.remove(&m.id).unwrap_or_default();
            }
        }
        Ok(rows)
    }

    /// Segna letto fino al messaggio indicato (mai all'indietro).
    pub fn mark_read(&self, conversation: i64, user: i64, message: i64) -> Result<()> {
        self.db().execute(
            "UPDATE members SET last_read = MAX(last_read, ?3) WHERE conversation_id = ?1 AND user_id = ?2",
            params![conversation, user, message],
        )?;
        Ok(())
    }

    // ---- file ----

    pub fn create_file(&self, uploader: i64, name: &str, mime: &str, size: u64) -> Result<String> {
        let id = random_id();
        self.db().execute(
            "INSERT INTO files (id, uploader_id, name, mime, size, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, uploader, name, mime, size as i64, now()],
        )?;
        Ok(id)
    }

    pub fn file(&self, id: &str) -> Result<Option<FileRow>> {
        Ok(self
            .db()
            .query_row(
                "SELECT id, uploader_id, name, mime, size, received, message_id FROM files WHERE id = ?1",
                [id],
                |r| {
                    Ok(FileRow {
                        id: r.get(0)?,
                        uploader_id: r.get(1)?,
                        name: r.get(2)?,
                        mime: r.get(3)?,
                        size: r.get::<_, i64>(4)? as u64,
                        received: r.get::<_, i64>(5)? as u64,
                        message_id: r.get(6)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn set_received(&self, id: &str, received: u64) -> Result<()> {
        self.db().execute("UPDATE files SET received = ?2 WHERE id = ?1", params![id, received as i64])?;
        Ok(())
    }

    /// Il file, se l'utente può scaricarlo: è suo oppure sta in una chat di cui fa parte.
    pub fn file_for(&self, id: &str, user: i64) -> Result<Option<FileRow>> {
        let Some(file) = self.file(id)? else { return Ok(None) };
        if file.uploader_id == user {
            return Ok(Some(file));
        }
        let Some(message) = file.message_id else { return Ok(None) };
        let allowed = self
            .db()
            .query_row(
                "SELECT 1 FROM messages m JOIN members mb ON mb.conversation_id = m.conversation_id
                 WHERE m.id = ?1 AND mb.user_id = ?2",
                params![message, user],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        Ok(allowed.then_some(file))
    }
}

fn user_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<User> {
    Ok(User { id: r.get(0)?, username: r.get(1)?, display_name: r.get(2)?, email: r.get(3)? })
}

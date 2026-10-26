//! Archivio locale SQLite: cartelle, intestazioni dei messaggi e messaggi
//! scaricati. Serve a mostrare subito la posta senza aspettare il server.

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

pub struct Store {
    conn: Mutex<Connection>,
}

/// Cartella come la descrive il server, pronta per l'archivio.
#[derive(Clone, Debug)]
pub struct FolderInfo {
    pub name: String,
    pub display: String,
    pub role: Option<&'static str>,
    pub depth: u32,
    pub selectable: bool,
    pub position: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub name: String,
    pub display: String,
    pub role: Option<String>,
    pub depth: u32,
    pub selectable: bool,
    pub unseen: u32,
    pub total: u32,
}

/// Intestazione di un messaggio letta dal server.
#[derive(Clone, Debug, Default)]
pub struct HeaderRow {
    pub uid: u32,
    pub message_id: String,
    pub subject: String,
    pub from_name: String,
    pub from_addr: String,
    pub to: String,
    pub cc: String,
    pub date: i64,
    pub seen: bool,
    pub flagged: bool,
    pub answered: bool,
    pub has_attachments: bool,
    pub size: u32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub uid: u32,
    pub subject: String,
    pub from_name: String,
    pub from_addr: String,
    pub to: String,
    pub date: i64,
    pub seen: bool,
    pub flagged: bool,
    pub answered: bool,
    pub has_attachments: bool,
    pub preview: String,
    pub size: u32,
}

/// Stato dei flag di un messaggio sul server.
pub struct FlagRow {
    pub uid: u32,
    pub seen: bool,
    pub flagged: bool,
    pub answered: bool,
}

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
CREATE TABLE IF NOT EXISTS folders (
    account     TEXT NOT NULL,
    name        TEXT NOT NULL,
    display     TEXT NOT NULL,
    role        TEXT,
    depth       INTEGER NOT NULL DEFAULT 0,
    selectable  INTEGER NOT NULL DEFAULT 1,
    position    INTEGER NOT NULL DEFAULT 0,
    uidvalidity INTEGER,
    unseen      INTEGER NOT NULL DEFAULT 0,
    total       INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (account, name)
);
CREATE TABLE IF NOT EXISTS messages (
    account         TEXT NOT NULL,
    folder          TEXT NOT NULL,
    uid             INTEGER NOT NULL,
    message_id      TEXT NOT NULL DEFAULT '',
    subject         TEXT NOT NULL DEFAULT '',
    from_name       TEXT NOT NULL DEFAULT '',
    from_addr       TEXT NOT NULL DEFAULT '',
    to_list         TEXT NOT NULL DEFAULT '',
    cc_list         TEXT NOT NULL DEFAULT '',
    date            INTEGER NOT NULL DEFAULT 0,
    seen            INTEGER NOT NULL DEFAULT 0,
    flagged         INTEGER NOT NULL DEFAULT 0,
    answered        INTEGER NOT NULL DEFAULT 0,
    has_attachments INTEGER NOT NULL DEFAULT 0,
    size            INTEGER NOT NULL DEFAULT 0,
    preview         TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (account, folder, uid)
);
CREATE INDEX IF NOT EXISTS messages_by_date ON messages (account, folder, date DESC);
CREATE TABLE IF NOT EXISTS bodies (
    account TEXT NOT NULL,
    folder  TEXT NOT NULL,
    uid     INTEGER NOT NULL,
    raw     BLOB NOT NULL,
    PRIMARY KEY (account, folder, uid)
);
CREATE TABLE IF NOT EXISTS trusted_senders (address TEXT PRIMARY KEY);
";

const SUMMARY_COLUMNS: &str = "uid, subject, from_name, from_addr, to_list, date, seen, flagged, answered, \
     has_attachments, preview, size";

fn summary_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Summary> {
    Ok(Summary {
        uid: r.get(0)?,
        subject: r.get(1)?,
        from_name: r.get(2)?,
        from_addr: r.get(3)?,
        to: r.get(4)?,
        date: r.get(5)?,
        seen: r.get(6)?,
        flagged: r.get(7)?,
        answered: r.get(8)?,
        has_attachments: r.get(9)?,
        preview: r.get(10)?,
        size: r.get(11)?,
    })
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    #[cfg(test)]
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn db(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    // ---- cartelle ----

    pub fn replace_folders(&self, account: &str, folders: &[FolderInfo]) -> Result<()> {
        let mut db = self.db();
        let tx = db.transaction()?;
        for f in folders {
            tx.execute(
                "INSERT INTO folders (account, name, display, role, depth, selectable, position)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT (account, name) DO UPDATE SET display = excluded.display,
                    role = excluded.role, depth = excluded.depth,
                    selectable = excluded.selectable, position = excluded.position",
                params![account, f.name, f.display, f.role, f.depth, f.selectable, f.position],
            )?;
        }
        let keep: HashSet<&str> = folders.iter().map(|f| f.name.as_str()).collect();
        let existing: Vec<String> = tx
            .prepare("SELECT name FROM folders WHERE account = ?1")?
            .query_map([account], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for name in existing.iter().filter(|n| !keep.contains(n.as_str())) {
            for table in ["folders", "messages", "bodies"] {
                let column = if table == "folders" { "name" } else { "folder" };
                tx.execute(
                    &format!("DELETE FROM {table} WHERE account = ?1 AND {column} = ?2"),
                    params![account, name],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn folders(&self, account: &str) -> Result<Vec<Folder>> {
        let db = self.db();
        let mut stmt = db.prepare(
            "SELECT name, display, role, depth, selectable, unseen, total FROM folders
             WHERE account = ?1 ORDER BY position, name COLLATE NOCASE",
        )?;
        let rows = stmt
            .query_map([account], |r| {
                Ok(Folder {
                    name: r.get(0)?,
                    display: r.get(1)?,
                    role: r.get(2)?,
                    depth: r.get(3)?,
                    selectable: r.get(4)?,
                    unseen: r.get(5)?,
                    total: r.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Nome della cartella con un certo ruolo ("sent", "trash", ...).
    pub fn folder_with_role(&self, account: &str, role: &str) -> Result<Option<String>> {
        Ok(self
            .db()
            .query_row(
                "SELECT name FROM folders WHERE account = ?1 AND role = ?2 AND selectable = 1
                 ORDER BY depth, name LIMIT 1",
                params![account, role],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn folder_role(&self, account: &str, folder: &str) -> Result<Option<String>> {
        Ok(self
            .db()
            .query_row(
                "SELECT role FROM folders WHERE account = ?1 AND name = ?2",
                params![account, folder],
                |r| r.get(0),
            )
            .optional()?
            .flatten())
    }

    /// Aggiorna i contatori; restituisce true se sono cambiati.
    pub fn set_counts(&self, account: &str, folder: &str, unseen: u32, total: u32) -> Result<bool> {
        let n = self.db().execute(
            "UPDATE folders SET unseen = ?3, total = ?4
             WHERE account = ?1 AND name = ?2 AND (unseen != ?3 OR total != ?4)",
            params![account, folder, unseen, total],
        )?;
        Ok(n > 0)
    }

    pub fn adjust_unseen(&self, account: &str, folder: &str, delta: i64) -> Result<()> {
        self.db().execute(
            "UPDATE folders SET unseen = MAX(0, unseen + ?3) WHERE account = ?1 AND name = ?2",
            params![account, folder, delta],
        )?;
        Ok(())
    }

    pub fn uidvalidity(&self, account: &str, folder: &str) -> Result<Option<u32>> {
        Ok(self
            .db()
            .query_row(
                "SELECT uidvalidity FROM folders WHERE account = ?1 AND name = ?2",
                params![account, folder],
                |r| r.get(0),
            )
            .optional()?
            .flatten())
    }

    /// Il server ha rinumerato la cartella: si butta la copia locale e si riparte.
    pub fn reset_folder(&self, account: &str, folder: &str, uidvalidity: u32) -> Result<()> {
        let mut db = self.db();
        let tx = db.transaction()?;
        tx.execute("DELETE FROM messages WHERE account = ?1 AND folder = ?2", params![account, folder])?;
        tx.execute("DELETE FROM bodies WHERE account = ?1 AND folder = ?2", params![account, folder])?;
        tx.execute(
            "UPDATE folders SET uidvalidity = ?3 WHERE account = ?1 AND name = ?2",
            params![account, folder, uidvalidity],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn clear_folder(&self, account: &str, folder: &str) -> Result<bool> {
        let db = self.db();
        let n = db.execute("DELETE FROM messages WHERE account = ?1 AND folder = ?2", params![account, folder])?;
        db.execute("DELETE FROM bodies WHERE account = ?1 AND folder = ?2", params![account, folder])?;
        Ok(n > 0)
    }

    // ---- messaggi ----

    pub fn max_uid(&self, account: &str, folder: &str) -> Result<Option<u32>> {
        Ok(self.db().query_row(
            "SELECT MAX(uid) FROM messages WHERE account = ?1 AND folder = ?2",
            params![account, folder],
            |r| r.get(0),
        )?)
    }

    pub fn min_uid(&self, account: &str, folder: &str) -> Result<Option<u32>> {
        Ok(self.db().query_row(
            "SELECT MIN(uid) FROM messages WHERE account = ?1 AND folder = ?2",
            params![account, folder],
            |r| r.get(0),
        )?)
    }

    pub fn count(&self, account: &str, folder: &str) -> Result<u32> {
        Ok(self.db().query_row(
            "SELECT COUNT(*) FROM messages WHERE account = ?1 AND folder = ?2",
            params![account, folder],
            |r| r.get(0),
        )?)
    }

    pub fn insert_headers(&self, account: &str, folder: &str, rows: &[HeaderRow]) -> Result<()> {
        let mut db = self.db();
        let tx = db.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO messages (account, folder, uid, message_id, subject, from_name, from_addr,
                    to_list, cc_list, date, seen, flagged, answered, has_attachments, size)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
                 ON CONFLICT (account, folder, uid) DO UPDATE SET seen = excluded.seen,
                    flagged = excluded.flagged, answered = excluded.answered",
            )?;
            for m in rows {
                stmt.execute(params![
                    account,
                    folder,
                    m.uid,
                    m.message_id,
                    m.subject,
                    m.from_name,
                    m.from_addr,
                    m.to,
                    m.cc,
                    m.date,
                    m.seen,
                    m.flagged,
                    m.answered,
                    m.has_attachments,
                    m.size
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Allinea i flag dei messaggi fra `lo` e `hi` a quelli del server e
    /// toglie quelli che sul server non ci sono più. Restituisce quante righe
    /// sono cambiate.
    pub fn sync_flags(&self, account: &str, folder: &str, lo: u32, hi: u32, flags: &[FlagRow]) -> Result<usize> {
        let mut db = self.db();
        let tx = db.transaction()?;
        let mut changed = 0;
        {
            let mut stmt = tx.prepare(
                "UPDATE messages SET seen = ?4, flagged = ?5, answered = ?6
                 WHERE account = ?1 AND folder = ?2 AND uid = ?3
                   AND (seen != ?4 OR flagged != ?5 OR answered != ?6)",
            )?;
            for f in flags {
                changed += stmt.execute(params![account, folder, f.uid, f.seen, f.flagged, f.answered])?;
            }
        }
        let present: HashSet<u32> = flags.iter().map(|f| f.uid).collect();
        let cached: Vec<u32> = tx
            .prepare("SELECT uid FROM messages WHERE account = ?1 AND folder = ?2 AND uid BETWEEN ?3 AND ?4")?
            .query_map(params![account, folder, lo, hi], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for uid in cached.into_iter().filter(|u| !present.contains(u)) {
            tx.execute(
                "DELETE FROM messages WHERE account = ?1 AND folder = ?2 AND uid = ?3",
                params![account, folder, uid],
            )?;
            tx.execute(
                "DELETE FROM bodies WHERE account = ?1 AND folder = ?2 AND uid = ?3",
                params![account, folder, uid],
            )?;
            changed += 1;
        }
        tx.commit()?;
        Ok(changed)
    }

    pub fn list(&self, account: &str, folder: &str, limit: u32, offset: u32) -> Result<Vec<Summary>> {
        let db = self.db();
        let mut stmt = db.prepare(&format!(
            "SELECT {SUMMARY_COLUMNS} FROM messages WHERE account = ?1 AND folder = ?2
             ORDER BY date DESC, uid DESC LIMIT ?3 OFFSET ?4"
        ))?;
        let rows = stmt
            .query_map(params![account, folder, limit, offset], summary_from_row)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn summary(&self, account: &str, folder: &str, uid: u32) -> Result<Option<Summary>> {
        Ok(self
            .db()
            .query_row(
                &format!("SELECT {SUMMARY_COLUMNS} FROM messages WHERE account = ?1 AND folder = ?2 AND uid = ?3"),
                params![account, folder, uid],
                summary_from_row,
            )
            .optional()?)
    }

    pub fn summaries(&self, account: &str, folder: &str, uids: &[u32]) -> Result<Vec<Summary>> {
        let mut out = Vec::new();
        for &uid in uids {
            out.extend(self.summary(account, folder, uid)?);
        }
        Ok(out)
    }

    pub fn set_flag(&self, account: &str, folder: &str, uid: u32, column: Flag, on: bool) -> Result<bool> {
        let column = match column {
            Flag::Seen => "seen",
            Flag::Flagged => "flagged",
            Flag::Answered => "answered",
        };
        let n = self.db().execute(
            &format!("UPDATE messages SET {column} = ?4 WHERE account = ?1 AND folder = ?2 AND uid = ?3 AND {column} != ?4"),
            params![account, folder, uid, on],
        )?;
        Ok(n > 0)
    }

    pub fn remove_message(&self, account: &str, folder: &str, uid: u32) -> Result<()> {
        let db = self.db();
        db.execute(
            "DELETE FROM messages WHERE account = ?1 AND folder = ?2 AND uid = ?3",
            params![account, folder, uid],
        )?;
        db.execute(
            "DELETE FROM bodies WHERE account = ?1 AND folder = ?2 AND uid = ?3",
            params![account, folder, uid],
        )?;
        Ok(())
    }

    pub fn body(&self, account: &str, folder: &str, uid: u32) -> Result<Option<Vec<u8>>> {
        Ok(self
            .db()
            .query_row(
                "SELECT raw FROM bodies WHERE account = ?1 AND folder = ?2 AND uid = ?3",
                params![account, folder, uid],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn put_body(&self, account: &str, folder: &str, uid: u32, raw: &[u8], preview: &str, has_attachments: bool) -> Result<()> {
        let mut db = self.db();
        let tx = db.transaction()?;
        tx.execute(
            "INSERT OR REPLACE INTO bodies (account, folder, uid, raw) VALUES (?1, ?2, ?3, ?4)",
            params![account, folder, uid, raw],
        )?;
        tx.execute(
            "UPDATE messages SET preview = ?4, has_attachments = ?5 WHERE account = ?1 AND folder = ?2 AND uid = ?3",
            params![account, folder, uid, preview, has_attachments],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Messaggi recenti non ancora scaricati e non troppo grandi, da
    /// scaricare in anticipo per avere l'anteprima e l'apertura immediata.
    pub fn uids_missing_body(&self, account: &str, folder: &str, max_size: u32, limit: u32) -> Result<Vec<u32>> {
        let db = self.db();
        let mut stmt = db.prepare(
            "SELECT m.uid FROM messages m
             LEFT JOIN bodies b ON b.account = m.account AND b.folder = m.folder AND b.uid = m.uid
             WHERE m.account = ?1 AND m.folder = ?2 AND b.uid IS NULL AND m.size <= ?3
             ORDER BY m.date DESC LIMIT ?4",
        )?;
        let rows = stmt
            .query_map(params![account, folder, max_size, limit], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn drop_account(&self, account: &str) -> Result<()> {
        let db = self.db();
        for table in ["folders", "messages", "bodies"] {
            db.execute(&format!("DELETE FROM {table} WHERE account = ?1"), [account])?;
        }
        Ok(())
    }

    // ---- mittenti fidati e rubrica ----

    pub fn is_trusted(&self, address: &str) -> Result<bool> {
        Ok(self
            .db()
            .query_row(
                "SELECT 1 FROM trusted_senders WHERE address = ?1",
                [address.to_lowercase()],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    pub fn trust(&self, address: &str) -> Result<()> {
        self.db().execute(
            "INSERT OR IGNORE INTO trusted_senders (address) VALUES (?1)",
            [address.to_lowercase()],
        )?;
        Ok(())
    }

    /// Indirizzi già visti che contengono il testo cercato, i più recenti prima.
    pub fn contacts(&self, query: &str, limit: u32) -> Result<Vec<(String, String)>> {
        let pattern = format!("%{}%", query.replace(['%', '_'], ""));
        let db = self.db();
        let mut stmt = db.prepare(
            "SELECT from_name, from_addr FROM messages
             WHERE from_addr != '' AND (from_addr LIKE ?1 OR from_name LIKE ?1)
             GROUP BY lower(from_addr) ORDER BY MAX(date) DESC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![pattern, limit], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }
}

#[derive(Clone, Copy)]
pub enum Flag {
    Seen,
    Flagged,
    Answered,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(uid: u32, date: i64) -> HeaderRow {
        HeaderRow { uid, date, subject: format!("m{uid}"), from_addr: format!("u{uid}@x.it"), ..Default::default() }
    }

    #[test]
    fn flags_sync_updates_and_removes() {
        let s = Store::in_memory().unwrap();
        s.insert_headers("a", "INBOX", &[row(1, 10), row(2, 20), row(3, 30)]).unwrap();
        assert_eq!((s.min_uid("a", "INBOX").unwrap(), s.max_uid("a", "INBOX").unwrap()), (Some(1), Some(3)));
        let flags = [
            FlagRow { uid: 1, seen: true, flagged: false, answered: false },
            FlagRow { uid: 3, seen: false, flagged: false, answered: false },
        ];
        // uid 1 diventa letto, uid 2 non c'è più sul server.
        assert_eq!(s.sync_flags("a", "INBOX", 1, 3, &flags).unwrap(), 2);
        let list = s.list("a", "INBOX", 10, 0).unwrap();
        assert_eq!(list.iter().map(|m| m.uid).collect::<Vec<_>>(), vec![3, 1]);
        assert!(list[1].seen);
    }

    #[test]
    fn missing_bodies_skip_downloaded_and_large() {
        let s = Store::in_memory().unwrap();
        let mut big = row(3, 30);
        big.size = 10_000_000;
        s.insert_headers("a", "INBOX", &[row(1, 10), row(2, 20), big]).unwrap();
        s.put_body("a", "INBOX", 2, b"raw", "anteprima", false).unwrap();
        assert_eq!(s.uids_missing_body("a", "INBOX", 2_000_000, 10).unwrap(), vec![1]);
        assert_eq!(s.summary("a", "INBOX", 2).unwrap().unwrap().preview, "anteprima");
    }
}

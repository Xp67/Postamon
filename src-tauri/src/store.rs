//! Archivio locale SQLite: cartelle, intestazioni dei messaggi e messaggi
//! scaricati. Serve a mostrare subito la posta senza aspettare il server.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use anyhow::{Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

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

/// Contatto della rubrica di una casella.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Contact {
    pub id: i64,
    /// Sempre in minuscolo: è la chiave con cui si riconoscono i mittenti.
    pub address: String,
    pub name: String,
    pub company: String,
    pub phone: String,
    pub notes: String,
    /// Quando è entrato in rubrica, in secondi.
    pub created: i64,
    /// Il dominio è di un'azienda, non di un servizio di posta comune.
    pub business: bool,
}

/// Scheda di un'azienda della rubrica. Le persone ci sono legate dal nome
/// scritto nel loro campo "Azienda" (senza badare a maiuscole e minuscole).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Company {
    pub id: i64,
    pub name: String,
    pub vat: String,
    pub phone: String,
    pub address: String,
    pub website: String,
    pub notes: String,
}

/// Scheda da salvare. Senza `id` l'azienda non ha ancora una scheda:
/// `previous_name` è il nome che hanno oggi le sue persone.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanyInput {
    #[serde(default)]
    pub id: Option<i64>,
    #[serde(default)]
    pub previous_name: String,
    pub name: String,
    #[serde(default)]
    pub vat: String,
    #[serde(default)]
    pub phone: String,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub website: String,
    #[serde(default)]
    pub notes: String,
}

/// Scheda appena salvata e quante persone hanno preso il nome nuovo.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedCompany {
    pub company: Company,
    pub contacts_renamed: usize,
}

/// Contatto appena salvato e quanti colleghi dello stesso dominio hanno
/// preso la sua azienda.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedContact {
    pub contact: Contact,
    pub company_shared: usize,
}

/// Esito di un'importazione nella rubrica.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    /// Contatti nuovi.
    pub added: usize,
    /// Contatti che c'erano già e a cui l'importazione ha riempito campi vuoti.
    pub completed: usize,
    /// Indirizzi non validi, lasciati fuori.
    pub skipped: usize,
}

/// Domini di posta di uso comune (e di PEC): chi li usa non dice niente della
/// sua azienda, quindi non legano fra loro i contatti.
const PUBLIC_DOMAINS: &[&str] = &[
    "gmail.com", "googlemail.com", "outlook.com", "outlook.it", "hotmail.com", "hotmail.it", "live.com", "live.it",
    "msn.com", "yahoo.com", "yahoo.it", "ymail.com", "icloud.com", "me.com", "mac.com", "aol.com", "gmx.com",
    "gmx.it", "gmx.net", "mail.com", "proton.me", "protonmail.com", "libero.it", "virgilio.it", "tiscali.it",
    "alice.it", "tim.it", "tin.it", "email.it", "inwind.it", "iol.it", "fastwebnet.it", "vodafone.it", "teletu.it",
    "katamail.com", "poste.it", "pec.it", "legalmail.it", "arubapec.it", "pec.aruba.it", "postacert.it",
    "postecert.it", "pecditta.com", "sicurezzapostale.it", "cert.legalmail.it",
];

/// Dominio dell'indirizzo, se è di un'azienda e non di un servizio di posta comune.
fn business_domain(address: &str) -> Option<&str> {
    let domain = address.rsplit_once('@')?.1;
    (domain.contains('.') && !PUBLIC_DOMAINS.contains(&domain)).then_some(domain)
}

/// Modello LIKE per "qualunque indirizzo di questo dominio".
fn domain_pattern(domain: &str) -> String {
    format!("%@{}", domain.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"))
}

/// Contatto da salvare: nuovo se `id` manca.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactInput {
    #[serde(default)]
    pub id: Option<i64>,
    pub address: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub company: String,
    #[serde(default)]
    pub phone: String,
    #[serde(default)]
    pub notes: String,
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
CREATE TABLE IF NOT EXISTS contacts (
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    account TEXT NOT NULL,
    address TEXT NOT NULL,
    name    TEXT NOT NULL DEFAULT '',
    company TEXT NOT NULL DEFAULT '',
    phone   TEXT NOT NULL DEFAULT '',
    notes   TEXT NOT NULL DEFAULT '',
    created INTEGER NOT NULL DEFAULT 0,
    UNIQUE (account, address)
);
CREATE TABLE IF NOT EXISTS companies (
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    account TEXT NOT NULL,
    name    TEXT NOT NULL COLLATE NOCASE,
    vat     TEXT NOT NULL DEFAULT '',
    phone   TEXT NOT NULL DEFAULT '',
    address TEXT NOT NULL DEFAULT '',
    website TEXT NOT NULL DEFAULT '',
    notes   TEXT NOT NULL DEFAULT '',
    UNIQUE (account, name)
);
";

const CONTACT_COLUMNS: &str = "id, address, name, company, phone, notes, created";

fn contact_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Contact> {
    let address: String = r.get(1)?;
    Ok(Contact {
        id: r.get(0)?,
        business: business_domain(&address).is_some(),
        address,
        name: r.get(2)?,
        company: r.get(3)?,
        phone: r.get(4)?,
        notes: r.get(5)?,
        created: r.get(6)?,
    })
}

const COMPANY_COLUMNS: &str = "id, name, vat, phone, address, website, notes";

fn company_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Company> {
    Ok(Company {
        id: r.get(0)?,
        name: r.get(1)?,
        vat: r.get(2)?,
        phone: r.get(3)?,
        address: r.get(4)?,
        website: r.get(5)?,
        notes: r.get(6)?,
    })
}

const SUMMARY_COLUMNS: &str = "uid, subject, from_name, from_addr, to_list, date, seen, flagged, answered, \
     has_attachments, preview, size";

fn summary_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Summary> {
    summary_at(r, 0)
}

/// `SUMMARY_COLUMNS` a partire dalla colonna `at`.
fn summary_at(r: &rusqlite::Row<'_>, at: usize) -> rusqlite::Result<Summary> {
    Ok(Summary {
        uid: r.get(at)?,
        subject: r.get(at + 1)?,
        from_name: r.get(at + 2)?,
        from_addr: r.get(at + 3)?,
        to: r.get(at + 4)?,
        date: r.get(at + 5)?,
        seen: r.get(at + 6)?,
        flagged: r.get(at + 7)?,
        answered: r.get(at + 8)?,
        has_attachments: r.get(at + 9)?,
        preview: r.get(at + 10)?,
        size: r.get(at + 11)?,
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

    /// Messaggi in archivio, i più recenti prima, come `(casella, cartella,
    /// messaggio)`. Le parole di `query` devono esserci tutte, ognuna in
    /// oggetto, mittente, destinatari o anteprima.
    pub fn search(
        &self,
        account: Option<&str>,
        folder: Option<&str>,
        query: &str,
        unread_only: bool,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<(String, String, Summary)>> {
        use rusqlite::types::Value;
        let mut sql = format!("SELECT account, folder, {SUMMARY_COLUMNS} FROM messages WHERE 1 = 1");
        let mut values: Vec<Value> = Vec::new();
        if let Some(account) = account {
            sql.push_str(" AND account = ?");
            values.push(Value::Text(account.to_string()));
        }
        if let Some(folder) = folder {
            sql.push_str(" AND folder = ?");
            values.push(Value::Text(folder.to_string()));
        }
        if unread_only {
            sql.push_str(" AND seen = 0");
        }
        for word in query.split_whitespace().take(8) {
            let pattern = format!("%{}%", word.replace(['%', '_'], ""));
            sql.push_str(
                " AND (subject LIKE ? OR from_name LIKE ? OR from_addr LIKE ? OR to_list LIKE ? \
                 OR cc_list LIKE ? OR preview LIKE ?)",
            );
            values.extend(std::iter::repeat_n(Value::Text(pattern), 6));
        }
        sql.push_str(" ORDER BY date DESC, uid DESC LIMIT ? OFFSET ?");
        values.push(Value::Integer(limit.into()));
        values.push(Value::Integer(offset.into()));
        let db = self.db();
        let mut stmt = db.prepare(&sql)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(values), |r| Ok((r.get(0)?, r.get(1)?, summary_at(r, 2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
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

    /// Toglie la copia della posta. La rubrica resta: è scritta a mano, e se
    /// la casella torna (stesso indirizzo, stesso id) ritrova i suoi contatti.
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

    /// Rubrica della casella, in ordine di nome.
    pub fn address_book(&self, account: &str) -> Result<Vec<Contact>> {
        let db = self.db();
        let mut stmt = db.prepare(&format!(
            "SELECT {CONTACT_COLUMNS} FROM contacts WHERE account = ?1
             ORDER BY CASE name WHEN '' THEN address ELSE name END COLLATE NOCASE"
        ))?;
        let rows = stmt.query_map([account], contact_from_row)?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Aggiunge o modifica un contatto. Un indirizzo compare una volta sola
    /// nella rubrica di una casella.
    ///
    /// Con un dominio aziendale l'azienda vale per tutto il dominio: i colleghi
    /// senza azienda, o con quella che il contatto aveva prima, prendono la
    /// nuova. Chi ha un'azienda diversa, scritta a mano, resta com'è.
    pub fn save_contact(&self, account: &str, input: &ContactInput) -> Result<SavedContact> {
        let address = input.address.trim().to_lowercase();
        if !address.contains('@') || address.contains(char::is_whitespace) {
            bail!("Indirizzo email non valido");
        }
        let id = input.id.unwrap_or(0);
        let mut db = self.db();
        let tx = db.transaction()?;
        let taken: Option<i64> = tx
            .query_row(
                "SELECT id FROM contacts WHERE account = ?1 AND address = ?2 AND id != ?3",
                params![account, address, id],
                |r| r.get(0),
            )
            .optional()?;
        if taken.is_some() {
            bail!("In rubrica c'è già un contatto con l'indirizzo {address}");
        }
        let (name, company, phone, notes) =
            (input.name.trim(), input.company.trim(), input.phone.trim(), input.notes.trim());
        let (id, old_company) = if input.id.is_some() {
            let old: Option<String> = tx
                .query_row(
                    "SELECT company FROM contacts WHERE account = ?1 AND id = ?2",
                    params![account, id],
                    |r| r.get(0),
                )
                .optional()?;
            let Some(old) = old else {
                bail!("Il contatto non è più in rubrica");
            };
            tx.execute(
                "UPDATE contacts SET address = ?3, name = ?4, company = ?5, phone = ?6, notes = ?7
                 WHERE account = ?1 AND id = ?2",
                params![account, id, address, name, company, phone, notes],
            )?;
            (id, old)
        } else {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            tx.execute(
                "INSERT INTO contacts (account, address, name, company, phone, notes, created)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![account, address, name, company, phone, notes, now],
            )?;
            (tx.last_insert_rowid(), String::new())
        };
        let company_shared = match business_domain(&address) {
            Some(domain) if !company.is_empty() => tx.execute(
                "UPDATE contacts SET company = ?1
                 WHERE account = ?2 AND id != ?3 AND address LIKE ?4 ESCAPE '\\'
                   AND company != ?1 AND (company = '' OR company = ?5)",
                params![company, account, id, domain_pattern(domain), old_company],
            )?,
            _ => 0,
        };
        // Se il vecchio nome non lo usa più nessuno, l'azienda è stata
        // rinominata: la sua scheda segue il nome nuovo.
        if !old_company.is_empty() && !company.is_empty() && old_company != company {
            let still_used: bool = tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM contacts WHERE account = ?1 AND company = ?2 COLLATE NOCASE)",
                params![account, old_company],
                |r| r.get(0),
            )?;
            if !still_used {
                tx.execute(
                    "UPDATE companies SET name = ?3 WHERE account = ?1 AND name = ?2
                       AND NOT EXISTS (SELECT 1 FROM companies WHERE account = ?1 AND name = ?3)",
                    params![account, old_company, company],
                )?;
            }
        }
        let contact = tx.query_row(
            &format!("SELECT {CONTACT_COLUMNS} FROM contacts WHERE id = ?1"),
            [id],
            contact_from_row,
        )?;
        tx.commit()?;
        Ok(SavedContact { contact, company_shared })
    }

    /// Azienda dei colleghi con lo stesso dominio aziendale, per proporla a
    /// un contatto nuovo: la più frequente, a parità la più recente.
    pub fn company_for(&self, account: &str, address: &str) -> Result<Option<String>> {
        let address = address.trim().to_lowercase();
        let Some(domain) = business_domain(&address) else {
            return Ok(None);
        };
        Ok(self
            .db()
            .query_row(
                "SELECT company FROM contacts
                 WHERE account = ?1 AND company != '' AND address LIKE ?2 ESCAPE '\\' AND address != ?3
                 GROUP BY company ORDER BY COUNT(*) DESC, MAX(id) DESC LIMIT 1",
                params![account, domain_pattern(domain), address],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Importa contatti da un'altra rubrica senza toccare quello che c'è già
    /// scritto: di un contatto già presente riempie solo i campi vuoti. Poi,
    /// come per un contatto salvato a mano, chi ha un dominio aziendale e
    /// nessuna azienda prende quella più frequente fra i colleghi.
    pub fn import_contacts(&self, account: &str, list: &[ContactInput]) -> Result<ImportReport> {
        let mut db = self.db();
        let tx = db.transaction()?;
        let snapshot = |tx: &rusqlite::Transaction<'_>| -> Result<HashMap<String, (String, String, String, String)>> {
            let mut stmt =
                tx.prepare("SELECT address, name, company, phone, notes FROM contacts WHERE account = ?1")?;
            let rows = stmt
                .query_map([account], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))))?
                .collect::<rusqlite::Result<_>>()?;
            Ok(rows)
        };
        let before = snapshot(&tx)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let mut report = ImportReport::default();
        let mut domains = HashSet::new();
        for input in list {
            let address = input.address.trim().to_lowercase();
            if !address.contains('@') || address.contains(char::is_whitespace) {
                report.skipped += 1;
                continue;
            }
            let (name, company, phone, notes) =
                (input.name.trim(), input.company.trim(), input.phone.trim(), input.notes.trim());
            let added = tx.execute(
                "INSERT INTO contacts (account, address, name, company, phone, notes, created)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) ON CONFLICT (account, address) DO NOTHING",
                params![account, address, name, company, phone, notes, now],
            )?;
            if added == 0 {
                tx.execute(
                    "UPDATE contacts SET
                       name = CASE name WHEN '' THEN ?3 ELSE name END,
                       company = CASE company WHEN '' THEN ?4 ELSE company END,
                       phone = CASE phone WHEN '' THEN ?5 ELSE phone END,
                       notes = CASE notes WHEN '' THEN ?6 ELSE notes END
                     WHERE account = ?1 AND address = ?2",
                    params![account, address, name, company, phone, notes],
                )?;
            }
            if let Some(domain) = business_domain(&address) {
                domains.insert(domain.to_string());
            }
        }
        for domain in &domains {
            tx.execute(
                "UPDATE contacts SET company = (
                   SELECT company FROM contacts
                   WHERE account = ?1 AND company != '' AND address LIKE ?2 ESCAPE '\\'
                   GROUP BY company ORDER BY COUNT(*) DESC, MAX(id) DESC LIMIT 1)
                 WHERE account = ?1 AND company = '' AND address LIKE ?2 ESCAPE '\\'
                   AND EXISTS (SELECT 1 FROM contacts
                               WHERE account = ?1 AND company != '' AND address LIKE ?2 ESCAPE '\\')",
                params![account, domain_pattern(domain)],
            )?;
        }
        for (address, row) in snapshot(&tx)? {
            match before.get(&address) {
                None => report.added += 1,
                Some(old) if *old != row => report.completed += 1,
                Some(_) => {}
            }
        }
        tx.commit()?;
        Ok(report)
    }

    pub fn delete_contact(&self, account: &str, id: i64) -> Result<()> {
        self.db().execute("DELETE FROM contacts WHERE account = ?1 AND id = ?2", params![account, id])?;
        Ok(())
    }

    /// Schede delle aziende della casella. Un'azienda che compare solo nel
    /// campo "Azienda" delle persone non ha ancora una scheda.
    pub fn companies(&self, account: &str) -> Result<Vec<Company>> {
        let db = self.db();
        let mut stmt =
            db.prepare(&format!("SELECT {COMPANY_COLUMNS} FROM companies WHERE account = ?1 ORDER BY name"))?;
        let rows = stmt.query_map([account], company_from_row)?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Salva la scheda di un'azienda. Se il nome cambia, lo prendono tutte le
    /// persone che avevano il vecchio.
    pub fn save_company(&self, account: &str, input: &CompanyInput) -> Result<SavedCompany> {
        let name = input.name.trim();
        if name.is_empty() {
            bail!("Scrivi il nome dell'azienda");
        }
        let mut db = self.db();
        let tx = db.transaction()?;
        // Scheda che si sta modificando, se c'è già, e nome che aveva.
        let existing: Option<(i64, String)> = match input.id {
            Some(id) => {
                let row = tx
                    .query_row(
                        "SELECT id, name FROM companies WHERE account = ?1 AND id = ?2",
                        params![account, id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?;
                if row.is_none() {
                    bail!("La scheda dell'azienda non c'è più");
                }
                row
            }
            None => tx
                .query_row(
                    "SELECT id, name FROM companies WHERE account = ?1 AND name = ?2",
                    params![account, input.previous_name.trim()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?,
        };
        let old_name = existing.as_ref().map_or(input.previous_name.trim().to_string(), |(_, n)| n.clone());
        let own_id = existing.as_ref().map_or(0, |(id, _)| *id);
        let clash: Option<i64> = tx
            .query_row(
                "SELECT id FROM companies WHERE account = ?1 AND name = ?2 AND id != ?3",
                params![account, name, own_id],
                |r| r.get(0),
            )
            .optional()?;
        if clash.is_some() {
            bail!("C'è già una scheda per l'azienda «{name}»");
        }
        let (vat, phone, address, website, notes) = (
            input.vat.trim(),
            input.phone.trim(),
            input.address.trim(),
            input.website.trim(),
            input.notes.trim(),
        );
        let id = if own_id != 0 {
            tx.execute(
                "UPDATE companies SET name = ?3, vat = ?4, phone = ?5, address = ?6, website = ?7, notes = ?8
                 WHERE account = ?1 AND id = ?2",
                params![account, own_id, name, vat, phone, address, website, notes],
            )?;
            own_id
        } else {
            tx.execute(
                "INSERT INTO companies (account, name, vat, phone, address, website, notes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![account, name, vat, phone, address, website, notes],
            )?;
            tx.last_insert_rowid()
        };
        let contacts_renamed = if !old_name.is_empty() && old_name != name {
            tx.execute(
                "UPDATE contacts SET company = ?3 WHERE account = ?1 AND company = ?2 COLLATE NOCASE",
                params![account, old_name, name],
            )?
        } else {
            0
        };
        let company = tx.query_row(
            &format!("SELECT {COMPANY_COLUMNS} FROM companies WHERE id = ?1"),
            [id],
            company_from_row,
        )?;
        tx.commit()?;
        Ok(SavedCompany { company, contacts_renamed })
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
    fn search_needs_every_word() {
        let s = Store::in_memory().unwrap();
        let mut a = row(1, 10);
        a.subject = "Preventivo divano Rossi".into();
        let mut b = row(2, 20);
        b.subject = "Preventivo poltrona".into();
        b.from_name = "Rossi Mobili".into();
        s.insert_headers("acc", "INBOX", &[a, b]).unwrap();
        s.insert_headers("altra", "INBOX", &[row(3, 30)]).unwrap();
        let uids = |found: Vec<(String, String, Summary)>| found.iter().map(|(_, _, m)| m.uid).collect::<Vec<_>>();
        assert_eq!(uids(s.search(None, None, "preventivo rossi", false, 10, 0).unwrap()), vec![2, 1]);
        assert_eq!(uids(s.search(None, None, "divano", false, 10, 0).unwrap()), vec![1]);
        assert_eq!(uids(s.search(Some("altra"), None, "", false, 10, 0).unwrap()), vec![3]);
        s.set_flag("acc", "INBOX", 2, Flag::Seen, true).unwrap();
        assert_eq!(uids(s.search(Some("acc"), Some("INBOX"), "", true, 10, 0).unwrap()), vec![1]);
    }

    #[test]
    fn address_book_per_account() {
        let s = Store::in_memory().unwrap();
        let input = |address: &str, name: &str| ContactInput {
            address: address.into(),
            name: name.into(),
            ..Default::default()
        };
        let luca = s.save_contact("a", &input(" Luca.Ferri@Ferrigomme.it ", "Luca Ferri")).unwrap().contact;
        assert_eq!(luca.address, "luca.ferri@ferrigomme.it");
        s.save_contact("a", &input("anna@x.it", "anna")).unwrap();
        s.save_contact("b", &input("luca.ferri@ferrigomme.it", "Luca")).unwrap();
        // Lo stesso indirizzo non entra due volte nella stessa rubrica.
        assert!(s.save_contact("a", &input("LUCA.FERRI@ferrigomme.it", "Doppione")).is_err());
        assert!(s.save_contact("a", &input("senza-chiocciola", "X")).is_err());
        let names = |list: Vec<Contact>| list.into_iter().map(|c| c.name).collect::<Vec<_>>();
        assert_eq!(names(s.address_book("a").unwrap()), vec!["anna", "Luca Ferri"]);

        let mut edit = input("luca@ferrigomme.it", "Luca Ferri");
        edit.id = Some(luca.id);
        edit.company = "Ferri Gomme".into();
        let edited = s.save_contact("a", &edit).unwrap().contact;
        assert_eq!((edited.address.as_str(), edited.company.as_str()), ("luca@ferrigomme.it", "Ferri Gomme"));
        // L'id di un'altra casella non si modifica da qui.
        edit.id = Some(s.address_book("b").unwrap()[0].id);
        edit.address = "nuovo@ferrigomme.it".into();
        assert!(s.save_contact("a", &edit).is_err());
        assert_eq!(s.address_book("b").unwrap()[0].address, "luca.ferri@ferrigomme.it");

        s.delete_contact("a", luca.id).unwrap();
        assert_eq!(names(s.address_book("a").unwrap()), vec!["anna"]);
        s.drop_account("b").unwrap();
        assert_eq!(s.address_book("b").unwrap().len(), 1);
    }

    #[test]
    fn company_follows_business_domain() {
        let s = Store::in_memory().unwrap();
        let save = |id: Option<i64>, address: &str, company: &str| {
            let input = ContactInput { id, address: address.into(), company: company.into(), ..Default::default() };
            s.save_contact("a", &input).unwrap()
        };
        let company = |address: &str| {
            s.address_book("a").unwrap().into_iter().find(|c| c.address == address).unwrap().company
        };
        let anna = save(None, "anna@ferrigomme.it", "Ferri Gomme");
        // Al nuovo collega si propone l'azienda del dominio.
        assert_eq!(s.company_for("a", "Paolo@FerriGomme.it").unwrap().as_deref(), Some("Ferri Gomme"));
        assert_eq!(s.company_for("b", "paolo@ferrigomme.it").unwrap(), None);
        save(None, "paolo@ferrigomme.it", "");
        save(None, "luca@ferrigomme.it", "");
        save(None, "a@altro-ferrigomme.it", "");

        // Rinominando l'azienda di Anna la prendono i colleghi senza azienda,
        // non chi ha un dominio solo simile.
        let renamed = save(Some(anna.contact.id), "anna@ferrigomme.it", "Ferri Gomme srl");
        assert_eq!(renamed.company_shared, 2);
        assert_eq!(company("paolo@ferrigomme.it"), "Ferri Gomme srl");
        assert_eq!(company("a@altro-ferrigomme.it"), "");
        // Chi ha un'azienda diversa, scritta a mano, non la passa a nessuno e non la perde.
        assert_eq!(save(None, "marta@ferrigomme.it", "Consulente esterna").company_shared, 0);
        // Un secondo rinomina da Paolo segue il vecchio nome anche su Anna e Luca.
        let paolo = s.address_book("a").unwrap().into_iter().find(|c| c.address.starts_with("paolo")).unwrap();
        assert_eq!(save(Some(paolo.id), "paolo@ferrigomme.it", "Ferri Gomme SpA").company_shared, 2);
        assert_eq!(company("anna@ferrigomme.it"), "Ferri Gomme SpA");
        assert_eq!(company("marta@ferrigomme.it"), "Consulente esterna");

        // I domini di uso comune non legano nessuno.
        save(None, "sara@gmail.com", "Studio Galli");
        let giulia = save(None, "giulia@gmail.com", "");
        assert_eq!(giulia.contact.company, "");
        assert_eq!(s.company_for("a", "nuovo@gmail.com").unwrap(), None);
        assert_eq!(save(Some(giulia.contact.id), "giulia@gmail.com", "Bar Giulia").company_shared, 0);
        assert_eq!(company("sara@gmail.com"), "Studio Galli");
    }

    #[test]
    fn company_cards_follow_their_people() {
        let s = Store::in_memory().unwrap();
        let person = |address: &str, company: &str| {
            let input = ContactInput { address: address.into(), company: company.into(), ..Default::default() };
            s.save_contact("a", &input).unwrap().contact
        };
        let anna = person("anna@ferrigomme.it", "Ferri Gomme");
        let sara = person("sara@gmail.com", "ferri gomme");
        assert!(anna.business && !sara.business);
        let card = |id: Option<i64>, previous: &str, name: &str| CompanyInput {
            id,
            previous_name: previous.into(),
            name: name.into(),
            vat: "IT01234567890".into(),
            ..Default::default()
        };
        // Prima scheda, creata dal nome che hanno le persone: rinominando la
        // prendono tutte, anche chi l'aveva scritto in minuscolo.
        let saved = s.save_company("a", &card(None, "Ferri Gomme", "Ferri Gomme srl")).unwrap();
        assert_eq!(saved.contacts_renamed, 2);
        let companies = |list: Vec<Contact>| list.into_iter().map(|c| c.company).collect::<Vec<_>>();
        assert_eq!(companies(s.address_book("a").unwrap()), vec!["Ferri Gomme srl", "Ferri Gomme srl"]);
        assert!(s.save_company("a", &card(None, "", "  ")).is_err());
        // Un'altra scheda non può prendere lo stesso nome.
        let other = s.save_company("a", &card(None, "", "Tappezzeria Ronchi")).unwrap().company;
        assert!(s.save_company("a", &card(Some(other.id), "", "FERRI GOMME SRL")).is_err());
        // Rinominata dalla scheda di una persona, la scheda segue il nome nuovo.
        let edit = ContactInput { id: Some(anna.id), address: anna.address, company: "Ferri Gomme SpA".into(), ..Default::default() };
        assert_eq!(s.save_contact("a", &edit).unwrap().company_shared, 0);
        let edit = ContactInput { id: Some(sara.id), address: sara.address, company: "Ferri Gomme SpA".into(), ..Default::default() };
        s.save_contact("a", &edit).unwrap();
        let names = s.companies("a").unwrap().into_iter().map(|c| c.name).collect::<Vec<_>>();
        assert_eq!(names, vec!["Ferri Gomme SpA", "Tappezzeria Ronchi"]);
        assert_eq!(s.companies("a").unwrap()[0].vat, "IT01234567890");
    }

    #[test]
    fn import_fills_only_empty_fields() {
        let s = Store::in_memory().unwrap();
        let input = |address: &str, name: &str, company: &str, phone: &str| ContactInput {
            address: address.into(),
            name: name.into(),
            company: company.into(),
            phone: phone.into(),
            ..Default::default()
        };
        // Già in rubrica: il nome scritto a mano resta, il telefono vuoto si riempie.
        s.save_contact("a", &input("anna@ferrigomme.it", "Anna", "", "")).unwrap();
        s.save_contact("a", &input("elena@libero.it", "Elena Rizzo", "", "")).unwrap();
        let report = s
            .import_contacts(
                "a",
                &[
                    input(" Anna@FerriGomme.it ", "Anna Ferri", "", "339 1"),
                    input("elena@libero.it", "Elena R.", "", ""),
                    input("paolo@ferrigomme.it", "Paolo", "Ferri Gomme", ""),
                    input("paolo@ferrigomme.it", "Paolo Doppio", "", "02 2"),
                    input("non è un indirizzo", "", "", ""),
                ],
            )
            .unwrap();
        assert_eq!(report, ImportReport { added: 1, completed: 1, skipped: 1 });
        let book = s.address_book("a").unwrap();
        let row = |addr: &str| book.iter().find(|c| c.address == addr).unwrap();
        let anna = row("anna@ferrigomme.it");
        assert_eq!((anna.name.as_str(), anna.phone.as_str()), ("Anna", "339 1"));
        assert_eq!(anna.company, "Ferri Gomme", "prende l'azienda del collega");
        assert_eq!(row("elena@libero.it").name, "Elena Rizzo");
        let paolo = row("paolo@ferrigomme.it");
        assert_eq!((paolo.name.as_str(), paolo.phone.as_str()), ("Paolo", "02 2"));
        // Una seconda importazione uguale non cambia niente.
        let again = s.import_contacts("a", &[input("paolo@ferrigomme.it", "Paolo", "Ferri Gomme", "")]).unwrap();
        assert_eq!(again, ImportReport::default());
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

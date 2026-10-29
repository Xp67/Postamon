//! Rubrica di Outlook classico, per importarla in quella di Postamon.
//!
//! Outlook si legge via COM con `outlook.ps1` (PowerShell c'è su ogni Windows,
//! niente dipendenze in più). Se Outlook non era aperto lo script lo avvia
//! senza finestre; alla fine va richiuso da qui con Restart Manager, perché
//! `Quit()` lascia vivo un Outlook avviato in background, e un Outlook nascosto
//! continuerebbe a scaricare posta e ad applicare le sue regole.

use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Deserializer, Serialize};

const SCRIPT: &str = include_str!("outlook.ps1");
/// Outlook può fermarsi su una domanda (profilo, password, permesso di
/// accedere ai contatti): si aspetta che l'utente risponda.
const TIMEOUT: Duration = Duration::from_secs(180);

/// Cartelle di contatti di Outlook, già pronte da importare.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlookBook {
    pub folders: Vec<OutlookFolder>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlookFolder {
    /// Casella di Outlook che contiene la cartella (di solito il suo indirizzo).
    pub store: String,
    pub name: String,
    /// Una persona per indirizzo email.
    pub people: Vec<OutlookPerson>,
    /// Contatti senza email: la rubrica di Postamon non li può tenere.
    pub without_email: usize,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlookPerson {
    pub name: String,
    pub address: String,
    pub company: String,
    pub phone: String,
    pub notes: String,
    /// Gruppo di Outlook da cui arriva, se non è anche un contatto.
    pub group: Option<String>,
}

// ---- risultato dello script ----

fn or_default<'de, D: Deserializer<'de>, T: Default + Deserialize<'de>>(d: D) -> Result<T, D::Error> {
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Raw {
    #[serde(deserialize_with = "or_default")]
    folders: Vec<RawFolder>,
    close_pid: u32,
    #[serde(deserialize_with = "or_default")]
    error: String,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct RawFolder {
    #[serde(deserialize_with = "or_default")]
    store: String,
    #[serde(deserialize_with = "or_default")]
    name: String,
    #[serde(deserialize_with = "or_default")]
    contacts: Vec<RawContact>,
    #[serde(deserialize_with = "or_default")]
    groups: Vec<RawGroup>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawContact {
    #[serde(deserialize_with = "or_default")]
    full_name: String,
    #[serde(deserialize_with = "or_default")]
    file_as: String,
    #[serde(deserialize_with = "or_default")]
    company: String,
    #[serde(deserialize_with = "or_default")]
    emails: Vec<RawEmail>,
    #[serde(deserialize_with = "or_default")]
    phones: Vec<String>,
    #[serde(deserialize_with = "or_default")]
    notes: String,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct RawEmail {
    #[serde(deserialize_with = "or_default")]
    address: String,
    #[serde(deserialize_with = "or_default")]
    display: String,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct RawGroup {
    #[serde(deserialize_with = "or_default")]
    name: String,
    #[serde(deserialize_with = "or_default")]
    members: Vec<RawMember>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct RawMember {
    #[serde(deserialize_with = "or_default")]
    name: String,
    #[serde(deserialize_with = "or_default")]
    address: String,
}

fn valid_address(address: &str) -> bool {
    address.contains('@') && !address.contains(char::is_whitespace)
}

/// Nome da mostrare: Outlook mette spesso l'indirizzo fra parentesi
/// ("Anna Ferrari (anna@x.it)"), o l'indirizzo stesso al posto del nome.
fn clean_name(name: &str, address: &str) -> String {
    let mut name = name.trim();
    if let Some(open) = name.rfind('(')
        && name.ends_with(')')
        && name[open + 1..name.len() - 1].trim().eq_ignore_ascii_case(address)
    {
        name = name[..open].trim_end();
    }
    if name.eq_ignore_ascii_case(address) { String::new() } else { name.to_string() }
}

fn convert(raw: Vec<RawFolder>) -> Vec<OutlookFolder> {
    raw.into_iter()
        .map(|f| {
            let mut people: Vec<OutlookPerson> = Vec::new();
            let mut seen = HashSet::new();
            let mut without_email = 0;
            for c in f.contacts {
                let mut phones: Vec<&str> = Vec::new();
                for p in c.phones.iter().map(|p| p.trim()).filter(|p| !p.is_empty()) {
                    if !phones.contains(&p) {
                        phones.push(p);
                    }
                }
                let mut any = false;
                for e in &c.emails {
                    let address = e.address.trim().to_lowercase();
                    if !valid_address(&address) || !seen.insert(address.clone()) {
                        continue;
                    }
                    any = true;
                    let name = [&c.full_name, &c.file_as, &e.display]
                        .into_iter()
                        .map(|n| clean_name(n, &address))
                        .find(|n| !n.is_empty())
                        .unwrap_or_default();
                    people.push(OutlookPerson {
                        name,
                        address,
                        company: c.company.trim().to_string(),
                        phone: phones.join(" · "),
                        notes: c.notes.trim().to_string(),
                        group: None,
                    });
                }
                if !any && c.emails.is_empty() {
                    without_email += 1;
                }
            }
            // Chi è solo in un gruppo arriva con il nome che ha lì.
            for g in f.groups {
                for m in g.members {
                    let address = m.address.trim().to_lowercase();
                    if !valid_address(&address) || !seen.insert(address.clone()) {
                        continue;
                    }
                    people.push(OutlookPerson {
                        name: clean_name(&m.name, &address),
                        address,
                        company: String::new(),
                        phone: String::new(),
                        notes: String::new(),
                        group: Some(g.name.trim().to_string()),
                    });
                }
            }
            people.sort_by_cached_key(|p| {
                if p.name.is_empty() { p.address.clone() } else { p.name.to_lowercase() }
            });
            OutlookFolder { store: f.store.trim().to_string(), name: f.name.trim().to_string(), people, without_email }
        })
        .filter(|f| !f.people.is_empty() || f.without_email > 0)
        .collect()
}

/// Legge la rubrica di Outlook. `work` è una cartella temporanea di Postamon.
pub async fn read(work: &Path) -> Result<OutlookBook> {
    if !cfg!(windows) {
        bail!("Importare da Outlook si può solo su Windows");
    }
    std::fs::create_dir_all(work)?;
    let script = work.join("outlook-rubrica.ps1");
    let out = work.join("outlook-rubrica.json");
    let _ = std::fs::remove_file(&out);
    // Con il BOM PowerShell 5 legge lo script come UTF-8 (lettere accentate nei commenti).
    std::fs::write(&script, format!("\u{feff}{SCRIPT}"))?;
    let mut cmd = tokio::process::Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script)
        .arg("-Out")
        .arg(&out)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let run = cmd.output();
    let output = match tokio::time::timeout(TIMEOUT, run).await {
        Ok(output) => output.context("Avvio di PowerShell non riuscito")?,
        Err(_) => bail!(
            "Outlook non ha risposto entro 3 minuti. Se ha aperto una finestra (profilo, password, permesso di \
             accedere ai contatti), rispondi e riprova"
        ),
    };
    let _ = std::fs::remove_file(&script);
    if output.status.code() == Some(3) {
        bail!("Su questo PC non c'è Outlook classico: il nuovo Outlook non permette di leggere la rubrica");
    }
    let text = std::fs::read_to_string(&out);
    let _ = std::fs::remove_file(&out);
    let Ok(text) = text else {
        let err = String::from_utf8_lossy(&output.stderr);
        bail!("Lettura della rubrica di Outlook non riuscita: {}", err.trim());
    };
    let raw: Raw = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .context("Risposta di Outlook non comprensibile")?;
    if raw.close_pid != 0 {
        let pid = raw.close_pid;
        // In sottofondo: i contatti servono subito, Outlook può chiudersi con calma.
        tokio::task::spawn_blocking(move || {
            if let Err(e) = close_gracefully(pid) {
                eprintln!("Outlook non richiuso: {e:#}");
            }
        });
    }
    if !raw.error.is_empty() {
        bail!("Outlook ha dato un errore: {}", raw.error);
    }
    Ok(OutlookBook { folders: convert(raw.folders) })
}

/// Chiude un programma come fa Windows prima di un aggiornamento: gli chiede
/// di salvare e uscire, senza mai terminarlo a forza.
#[cfg(windows)]
fn close_gracefully(pid: u32) -> Result<()> {
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, FILETIME};
    use windows_sys::Win32::System::RestartManager::{
        CCH_RM_SESSION_KEY, RM_UNIQUE_PROCESS, RmEndSession, RmRegisterResources, RmShutdown, RmStartSession,
    };
    use windows_sys::Win32::System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    // SAFETY: handle aperto e chiuso qui; i puntatori sono a variabili locali.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return Ok(()); // già uscito
        }
        let ok = GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user);
        CloseHandle(process);
        if ok == 0 {
            bail!("ora di avvio del processo {pid} non leggibile");
        }
    }
    let app = RM_UNIQUE_PROCESS { dwProcessId: pid, ProcessStartTime: created };
    let mut session = 0u32;
    let mut key = [0u16; CCH_RM_SESSION_KEY as usize + 1];
    // SAFETY: buffer della chiave lungo quanto chiede l'API; un solo processo registrato.
    unsafe {
        let r = RmStartSession(&mut session, 0, key.as_mut_ptr());
        if r != ERROR_SUCCESS {
            bail!("Restart Manager non disponibile ({r})");
        }
        let mut r = RmRegisterResources(session, 0, std::ptr::null(), 1, &app, 0, std::ptr::null());
        if r == ERROR_SUCCESS {
            // 0 = solo chiusura ordinata: se il programma rifiuta, resta aperto.
            r = RmShutdown(session, 0, None);
        }
        RmEndSession(session);
        if r != ERROR_SUCCESS {
            bail!("chiusura rifiutata ({r})");
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn close_gracefully(_pid: u32) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"folders":[{"store":"marco@ds.it","name":"Contatti (solo questo computer)","contacts":[
        {"fullName":"Anna Ferrari","fileAs":"Ferrari, Anna","company":"Arredamenti Ferrari",
         "emails":[{"address":"Anna.Ferrari@AF.it","display":"Anna Ferrari (anna.ferrari@af.it)"},
                   {"address":"anna@gmail.com","display":"anna@gmail.com"}],
         "phones":["339 555 7710","0331 555 200","339 555 7710"],"notes":" Preferisce il mattino \r\n"},
        {"fullName":"","fileAs":"","company":"","emails":[{"address":"ordini@tr.it","display":"Ufficio ordini (ordini@tr.it)"}],
         "phones":[],"notes":""},
        {"fullName":"Mario Bianchi","fileAs":"","company":"","emails":[],"phones":["02 1234"],"notes":""},
        {"fullName":"Doppio","fileAs":"","company":"","emails":[{"address":"anna@gmail.com","display":""}],"phones":null,"notes":null}],
      "groups":[{"name":"Fornitori","members":[{"name":"ordini@tr.it","address":"ordini@tr.it"},
                                                {"name":"Luca Verdi","address":"luca@verdi.it"},
                                                {"name":"Interno","address":"/o=ExchangeLabs/cn=x"}]}]},
      {"store":"info@ds.it","name":"Contatti","contacts":[],"groups":[]}],
      "closePid":0,"error":""}"#;

    #[test]
    fn outlook_contacts_become_people() {
        let raw: Raw = serde_json::from_str(SAMPLE).unwrap();
        let folders = convert(raw.folders);
        assert_eq!(folders.len(), 1, "le cartelle vuote non compaiono");
        let f = &folders[0];
        assert_eq!(f.without_email, 1);
        let rows: Vec<_> = f.people.iter().map(|p| (p.name.as_str(), p.address.as_str())).collect();
        assert_eq!(
            rows,
            [
                ("Anna Ferrari", "anna.ferrari@af.it"),
                ("Anna Ferrari", "anna@gmail.com"),
                ("Luca Verdi", "luca@verdi.it"),
                ("Ufficio ordini", "ordini@tr.it"),
            ]
        );
        let anna = &f.people[0];
        assert_eq!(anna.company, "Arredamenti Ferrari");
        assert_eq!(anna.phone, "339 555 7710 · 0331 555 200");
        assert_eq!(anna.notes, "Preferisce il mattino");
        assert_eq!(anna.group, None);
        assert_eq!(f.people[2].group.as_deref(), Some("Fornitori"));
        assert_eq!(f.people[3].group, None, "chi è anche un contatto non arriva dal gruppo");
    }

    #[test]
    fn names_without_addresses() {
        assert_eq!(clean_name("Anna (Anna@X.it)", "anna@x.it"), "Anna");
        assert_eq!(clean_name("Anna (ufficio)", "anna@x.it"), "Anna (ufficio)");
        assert_eq!(clean_name("ANNA@X.IT", "anna@x.it"), "");
        assert_eq!(clean_name(" Anna Ferrari ", "anna@x.it"), "Anna Ferrari");
    }
}

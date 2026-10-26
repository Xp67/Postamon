//! Accesso, in quest'ordine:
//! - utenti di prova dalla configurazione;
//! - indirizzo email e password della casella, verificati con un accesso IMAP
//!   al server di posta (la password non si conserva);
//! - account del dominio verificati con il controller di dominio (bind LDAP);
//!   in questo caso si legge anche l'elenco dei colleghi.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use ldap3::{LdapConnAsync, LdapConnSettings, Scope, SearchEntry};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::config::{Config, Imap, Ldap};

pub struct Person {
    pub username: String,
    pub display_name: String,
    pub email: String,
}

pub struct Login {
    pub person: Person,
    pub source: &'static str,
    /// Colleghi letti dal dominio (vuoto per gli utenti di prova).
    pub directory: Vec<Person>,
}

#[derive(Debug)]
pub enum AuthError {
    /// Nome o password sbagliati.
    Invalid,
    /// Il controller di dominio non risponde.
    Unavailable(String),
}

/// Identità con cui si salva l'utente: l'indirizzo email intero
/// ("Marco@Dimensionesalotto.it" → "marco@dimensionesalotto.it"), oppure il
/// nome senza dominio Windows ("AZIENDA\\Marco" → "marco").
pub fn normalize(input: &str) -> String {
    let name = input.trim();
    if name.contains('@') {
        return name.to_lowercase();
    }
    name.rsplit('\\').next().unwrap_or(name).trim().to_lowercase()
}

pub async fn login(cfg: &Config, raw_username: &str, password: &str, display_name: &str) -> Result<Login, AuthError> {
    let username = normalize(raw_username);
    if username.is_empty() || password.is_empty() {
        return Err(AuthError::Invalid);
    }
    if let Some(local) = cfg.local_users.iter().find(|u| normalize(&u.username) == username) {
        if constant_time_eq(local.password.as_bytes(), password.as_bytes()) {
            return Ok(Login {
                person: Person { username, display_name: local.display_name.clone(), email: String::new() },
                source: "local",
                directory: Vec::new(),
            });
        }
        return Err(AuthError::Invalid);
    }
    if let (Some(imap), Some((_, domain))) = (&cfg.imap, username.split_once('@')) {
        if !imap.allowed_domains.is_empty() && !imap.allowed_domains.iter().any(|d| d.eq_ignore_ascii_case(domain)) {
            return Err(AuthError::Invalid);
        }
        imap_login(imap, &username, password).await?;
        let display = display_name.trim();
        let display = if display.is_empty() { name_from_email(&username) } else { display.to_string() };
        return Ok(Login {
            person: Person { display_name: display, email: username.clone(), username },
            source: "mail",
            directory: Vec::new(),
        });
    }
    match &cfg.ldap {
        Some(ldap) => {
            let sam = username.split('@').next().unwrap_or(&username).to_string();
            ldap_login(ldap, &sam, password).await
        }
        None => Err(AuthError::Invalid),
    }
}

/// "mario.rossi@x.it" → "Mario Rossi"
fn name_from_email(email: &str) -> String {
    email
        .split('@')
        .next()
        .unwrap_or(email)
        .split(['.', '_', '-'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Verifica email e password con un accesso IMAP (AUTHENTICATE PLAIN su TLS)
/// e si scollega subito.
async fn imap_login(cfg: &Imap, email: &str, password: &str) -> Result<(), AuthError> {
    let unavailable = |e: String| AuthError::Unavailable(format!("{}: {e}", cfg.host));
    let tcp = tokio::time::timeout(Duration::from_secs(10), tokio::net::TcpStream::connect((cfg.host.as_str(), cfg.port)))
        .await
        .map_err(|_| unavailable("tempo scaduto".into()))?
        .map_err(|e| unavailable(e.to_string()))?;
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|e| unavailable(e.to_string()))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let name = rustls::pki_types::ServerName::try_from(cfg.host.clone()).map_err(|e| unavailable(e.to_string()))?;
    let tls = tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect(name, tcp)
        .await
        .map_err(|e| unavailable(e.to_string()))?;
    let (read, mut write) = tokio::io::split(tls);
    let mut lines = BufReader::new(read).lines();
    let mut next = async || -> Result<String, AuthError> {
        match tokio::time::timeout(Duration::from_secs(15), lines.next_line()).await {
            Ok(Ok(Some(line))) => Ok(line),
            Ok(Ok(None)) => Err(unavailable("connessione chiusa".into())),
            Ok(Err(e)) => Err(unavailable(e.to_string())),
            Err(_) => Err(unavailable("tempo scaduto".into())),
        }
    };
    let greeting = next().await?;
    if !greeting.starts_with("* OK") {
        return Err(unavailable(greeting));
    }
    let token = base64::engine::general_purpose::STANDARD.encode(format!("\0{email}\0{password}"));
    write
        .write_all(format!("a1 AUTHENTICATE PLAIN {token}\r\n").as_bytes())
        .await
        .map_err(|e| unavailable(e.to_string()))?;
    loop {
        let line = next().await?;
        if line.starts_with('+') {
            // Server senza SASL-IR: vuole le credenziali su una riga a parte.
            write.write_all(format!("{token}\r\n").as_bytes()).await.map_err(|e| unavailable(e.to_string()))?;
            continue;
        }
        if let Some(status) = line.strip_prefix("a1 ") {
            let _ = write.write_all(b"a2 LOGOUT\r\n").await;
            return if status.starts_with("OK") {
                Ok(())
            } else if status.starts_with("NO") || status.starts_with("BAD") {
                Err(AuthError::Invalid)
            } else {
                Err(unavailable(line))
            };
        }
    }
}

async fn ldap_login(cfg: &Ldap, username: &str, password: &str) -> Result<Login, AuthError> {
    let unavailable = |e: ldap3::LdapError| AuthError::Unavailable(e.to_string());
    let settings = LdapConnSettings::new()
        .set_conn_timeout(Duration::from_secs(8))
        .set_no_tls_verify(!cfg.verify_tls);
    let (conn, mut ldap) = LdapConnAsync::with_settings(settings, &cfg.url).await.map_err(unavailable)?;
    ldap3::drive!(conn);
    let upn = format!("{username}@{}", cfg.upn_suffix);
    let bind = ldap.simple_bind(&upn, password).await.map_err(unavailable)?;
    if bind.rc != 0 {
        // 49 = credenziali non valide (anche account disabilitato o scaduto).
        return Err(AuthError::Invalid);
    }
    let attrs = vec!["sAMAccountName", "displayName", "mail"];
    let me = ldap
        .search(&cfg.base_dn, Scope::Subtree, &format!("(sAMAccountName={})", escape(username)), attrs.clone())
        .await
        .and_then(|r| r.success())
        .map_err(unavailable)?
        .0
        .into_iter()
        .map(SearchEntry::construct)
        .next()
        .map(person)
        .unwrap_or(Person { username: username.to_string(), display_name: username.to_string(), email: String::new() });
    // Persone con account attivo (il bit 2 di userAccountControl è "disabilitato").
    let filter = "(&(objectCategory=person)(objectClass=user)(!(userAccountControl:1.2.840.113556.1.4.803:=2)))";
    let directory = ldap
        .search(&cfg.base_dn, Scope::Subtree, filter, attrs)
        .await
        .and_then(|r| r.success())
        .map(|(entries, _)| entries.into_iter().map(SearchEntry::construct).map(person).collect())
        .unwrap_or_default();
    let _ = ldap.unbind().await;
    Ok(Login { person: Person { username: username.to_string(), ..me }, source: "ad", directory })
}

fn person(entry: SearchEntry) -> Person {
    let first = |name: &str| entry.attrs.get(name).and_then(|v| v.first()).cloned().unwrap_or_default();
    let username = first("sAMAccountName").to_lowercase();
    let display = first("displayName");
    Person {
        display_name: if display.trim().is_empty() { username.clone() } else { display },
        email: first("mail"),
        username,
    }
}

/// Caratteri speciali dei filtri LDAP (RFC 4515).
fn escape(value: &str) -> String {
    let mut out = String::new();
    for c in value.chars() {
        match c {
            '*' => out.push_str("\\2a"),
            '(' => out.push_str("\\28"),
            ')' => out.push_str("\\29"),
            '\\' => out.push_str("\\5c"),
            '\0' => out.push_str("\\00"),
            c => out.push(c),
        }
    }
    out
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Dopo 5 password sbagliate di fila un nome resta bloccato per un minuto.
#[derive(Default)]
pub struct Throttle {
    failures: Mutex<HashMap<String, (u32, Instant)>>,
}

const MAX_FAILURES: u32 = 5;
const LOCKOUT: Duration = Duration::from_secs(60);

impl Throttle {
    pub fn locked(&self, username: &str) -> bool {
        let map = self.failures.lock().unwrap_or_else(|p| p.into_inner());
        map.get(username).is_some_and(|(n, at)| *n >= MAX_FAILURES && at.elapsed() < LOCKOUT)
    }

    pub fn failed(&self, username: &str) {
        let mut map = self.failures.lock().unwrap_or_else(|p| p.into_inner());
        let entry = map.entry(username.to_string()).or_insert((0, Instant::now()));
        if entry.1.elapsed() >= LOCKOUT {
            *entry = (0, Instant::now());
        }
        entry.0 += 1;
        entry.1 = Instant::now();
    }

    pub fn succeeded(&self, username: &str) {
        self.failures.lock().unwrap_or_else(|p| p.into_inner()).remove(username);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_usernames() {
        assert_eq!(normalize("AZIENDA\\Marco"), "marco");
        assert_eq!(normalize(" Marco@DimensioneSalotto.it "), "marco@dimensionesalotto.it");
        assert_eq!(normalize("Marco"), "marco");
    }

    #[test]
    fn names_from_emails() {
        assert_eq!(name_from_email("mario.rossi@x.it"), "Mario Rossi");
        assert_eq!(name_from_email("info@x.it"), "Info");
    }

    /// Prova contro il server di posta vero con un indirizzo inesistente:
    /// POSTAMON_IMAP_HOST=in.postassl.it cargo test rejects_wrong_mail_password
    #[tokio::test]
    async fn rejects_wrong_mail_password() {
        let Ok(host) = std::env::var("POSTAMON_IMAP_HOST") else { return };
        let _ = rustls::crypto::ring::default_provider().install_default();
        let cfg = Imap { host, port: 993, allowed_domains: vec![] };
        let r = imap_login(&cfg, "postamon-prova-inesistente@dimensionesalotto.it", "non-valida").await;
        assert!(matches!(r, Err(AuthError::Invalid)), "{r:?}");
    }

    #[test]
    fn escapes_ldap_filters() {
        assert_eq!(escape("a*b(c)\\"), "a\\2ab\\28c\\29\\5c");
    }

    #[test]
    fn throttles_after_five_failures() {
        let t = Throttle::default();
        for _ in 0..4 {
            t.failed("x");
        }
        assert!(!t.locked("x"));
        t.failed("x");
        assert!(t.locked("x"));
        t.succeeded("x");
        assert!(!t.locked("x"));
    }
}

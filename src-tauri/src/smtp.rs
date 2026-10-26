//! Composizione e invio dei messaggi via SMTP.

use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use lettre::message::header::ContentType;
use lettre::message::{Attachment, Mailbox, MultiPart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::accounts::Account;
use crate::render;

pub struct OutgoingFile {
    pub name: String,
    pub mime: String,
    pub data: Vec<u8>,
}

pub struct Draft<'a> {
    pub to: &'a str,
    pub cc: &'a str,
    pub bcc: &'a str,
    pub subject: &'a str,
    pub body: &'a str,
    pub files: Vec<OutgoingFile>,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
}

pub fn build(acc: &Account, draft: Draft<'_>) -> Result<Message> {
    let to = parse_recipients(draft.to)?;
    let cc = parse_recipients(draft.cc)?;
    let bcc = parse_recipients(draft.bcc)?;
    if to.is_empty() && cc.is_empty() && bcc.is_empty() {
        bail!("Manca il destinatario");
    }
    let name = Some(acc.name.trim().to_string()).filter(|n| !n.is_empty());
    let from = Mailbox::new(name, acc.email.parse().context("indirizzo del mittente non valido")?);
    let mut b = Message::builder().from(from).subject(draft.subject.trim()).user_agent("Postamon".into());
    for m in to {
        b = b.to(m);
    }
    for m in cc {
        b = b.cc(m);
    }
    for m in bcc {
        b = b.bcc(m);
    }
    if let Some(id) = draft.in_reply_to {
        b = b.in_reply_to(id);
    }
    if let Some(refs) = draft.references {
        b = b.references(refs);
    }
    let text = draft.body.replace("\r\n", "\n");
    let alternative = MultiPart::alternative_plain_html(text.clone(), text_to_html(&text));
    let message = if draft.files.is_empty() {
        b.multipart(alternative)?
    } else {
        let mut mixed = MultiPart::mixed().multipart(alternative);
        for f in draft.files {
            let ct = ContentType::parse(&f.mime).unwrap_or(ContentType::parse("application/octet-stream")?);
            mixed = mixed.singlepart(Attachment::new(f.name).body(f.data, ct));
        }
        b.multipart(mixed)?
    };
    Ok(message)
}

/// Divide "Mario <m@x.it>, \"Rossi, Anna\" <a@x.it>; b@x.it" sulle virgole e
/// sui punti e virgola che non stanno fra virgolette o fra < >.
pub fn parse_recipients(input: &str) -> Result<Vec<Mailbox>> {
    let mut items = Vec::new();
    let mut current = String::new();
    let (mut quoted, mut angle) = (false, false);
    for c in input.chars() {
        match c {
            '"' => quoted = !quoted,
            '<' if !quoted => angle = true,
            '>' if !quoted => angle = false,
            ',' | ';' if !quoted && !angle => {
                items.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    items.push(current);
    items
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<Mailbox>().map_err(|_| anyhow!("Indirizzo non valido: {s}")))
        .collect()
}

/// Versione HTML del testo scritto: stesso contenuto, carattere leggibile,
/// righe citate ("> ") in grigio come nei client più diffusi.
fn text_to_html(text: &str) -> String {
    let mut html = String::from(
        "<div style=\"font-family:'Segoe UI',Calibri,Arial,sans-serif;font-size:11pt;color:#1b1b1b\">",
    );
    for line in text.split('\n') {
        if let Some(quoted) = line.strip_prefix('>') {
            html.push_str(&format!(
                "<div style=\"color:#5f6b76;border-left:2px solid #c8d1d9;padding-left:8px\">{}</div>",
                nbsp_or(&render::linkify(quoted.strip_prefix(' ').unwrap_or(quoted)))
            ));
        } else {
            html.push_str(&format!("<div>{}</div>", nbsp_or(&render::linkify(line))));
        }
    }
    html.push_str("</div>");
    html
}

fn nbsp_or(s: &str) -> String {
    if s.trim().is_empty() { "&nbsp;".to_string() } else { s.to_string() }
}

fn transport(acc: &Account, password: &str) -> Result<AsyncSmtpTransport<Tokio1Executor>> {
    let tls = TlsParameters::builder(acc.smtp_host.clone())
        .dangerous_accept_invalid_certs(acc.accept_invalid_certs)
        .dangerous_accept_invalid_hostnames(acc.accept_invalid_certs)
        .build()?;
    // 587 e 25 sono le porte di STARTTLS (si parte in chiaro e si passa a
    // TLS); tutte le altre, 465 in testa, sono cifrate fin dal primo byte.
    let tls = if matches!(acc.smtp_port, 587 | 25) { Tls::Required(tls) } else { Tls::Wrapper(tls) };
    Ok(AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&acc.smtp_host)
        .port(acc.smtp_port)
        .tls(tls)
        .credentials(Credentials::new(acc.email.clone(), password.to_string()))
        .timeout(Some(Duration::from_secs(120)))
        .build())
}

/// Invia il messaggio e restituisce i byte spediti, da salvare in "Posta inviata".
pub async fn send(acc: &Account, password: &str, message: &Message) -> Result<Vec<u8>> {
    let raw = message.formatted();
    transport(acc, password)?
        .send_raw(message.envelope(), &raw)
        .await
        .map_err(|e| {
            if e.is_permanent() {
                anyhow!("Il server ha rifiutato il messaggio: {e}")
            } else {
                anyhow!("Invio non riuscito: {e}")
            }
        })?;
    Ok(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_recipients_outside_quotes() {
        let list = parse_recipients("Mario <m@x.it>, \"Rossi, Anna\" <a@x.it>; b@x.it,").unwrap();
        let emails: Vec<String> = list.iter().map(|m| m.email.to_string()).collect();
        assert_eq!(emails, vec!["m@x.it", "a@x.it", "b@x.it"]);
        assert_eq!(list[1].name.as_deref(), Some("Rossi, Anna"));
        assert!(parse_recipients("non-un-indirizzo").is_err());
    }

    #[test]
    fn builds_html_alternative_with_attachment() {
        let acc = Account {
            id: "m@x.it".into(),
            email: "m@x.it".into(),
            name: "Marco".into(),
            imap_host: String::new(),
            imap_port: 993,
            smtp_host: String::new(),
            smtp_port: 465,
            signature: String::new(),
            accept_invalid_certs: false,
        };
        let draft = Draft {
            to: "a@x.it",
            cc: "",
            bcc: "nascosto@x.it",
            subject: "Prova",
            body: "Ciao\n> citato https://x.it",
            files: vec![OutgoingFile { name: "nota.txt".into(), mime: "text/plain".into(), data: b"ok".to_vec() }],
            in_reply_to: Some("<abc@x.it>".into()),
            references: None,
        };
        let msg = build(&acc, draft).unwrap();
        let raw = String::from_utf8(msg.formatted()).unwrap();
        assert!(raw.contains("multipart/mixed"));
        assert!(raw.contains("multipart/alternative"));
        assert!(raw.contains("nota.txt"));
        assert!(raw.contains("In-Reply-To: <abc@x.it>"));
        // il Ccn non deve comparire nelle intestazioni spedite
        assert!(!raw.contains("nascosto@x.it"));
        assert_eq!(msg.envelope().to().len(), 2);
    }
}

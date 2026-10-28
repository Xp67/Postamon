//! Composizione e invio dei messaggi via SMTP.

use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use lettre::message::header::ContentType;
use lettre::message::{Attachment, Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::accounts::Account;
use crate::{render, signatures};

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
    /// Citazione o messaggio inoltrato, sotto la firma.
    pub quote: &'a str,
    /// HTML della firma (già ripulito, dall'elenco delle firme).
    pub signature: Option<&'a str>,
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
    // Testo, firma e citazione, separati da una riga vuota, in tutte e due le versioni.
    let mut text: Vec<String> = Vec::new();
    let mut html: Vec<String> = Vec::new();
    let mut images = Vec::new();
    let body = draft.body.replace("\r\n", "\n");
    if !body.trim().is_empty() {
        text.push(body.trim_end().to_string());
        html.push(lines_to_html(body.trim_end()));
    }
    if let Some(sig) = draft.signature.filter(|s| !s.trim().is_empty()) {
        let (sig_html, inline) = signatures::for_mail(sig);
        text.push(signatures::to_text(sig));
        html.push(format!("<div>{sig_html}</div>"));
        images = inline;
    }
    let quote = draft.quote.replace("\r\n", "\n");
    if !quote.trim().is_empty() {
        text.push(quote.trim_end().to_string());
        html.push(lines_to_html(quote.trim_end()));
    }
    let text = text.join("\n\n");
    let html = format!(
        "<div style=\"font-family:'Segoe UI',Calibri,Arial,sans-serif;font-size:11pt;color:#1b1b1b\">{}</div>",
        html.join("<div>&nbsp;</div>")
    );
    let alternative = if images.is_empty() {
        MultiPart::alternative_plain_html(text, html)
    } else {
        // Le immagini della firma viaggiano col messaggio, legate all'HTML con il loro cid.
        let mut related = MultiPart::related().singlepart(SinglePart::html(html));
        for img in images {
            let ct = ContentType::parse(&img.mime)?;
            related = related.singlepart(Attachment::new_inline(img.cid).body(img.data, ct));
        }
        MultiPart::alternative().singlepart(SinglePart::plain(text)).multipart(related)
    };
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

/// Versione HTML del testo scritto: stesse righe, link cliccabili, righe
/// citate ("> ") in grigio come nei client più diffusi.
fn lines_to_html(text: &str) -> String {
    let mut html = String::new();
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
    use mail_parser::MimeHeaders;

    use super::*;

    #[test]
    fn splits_recipients_outside_quotes() {
        let list = parse_recipients("Mario <m@x.it>, \"Rossi, Anna\" <a@x.it>; b@x.it,").unwrap();
        let emails: Vec<String> = list.iter().map(|m| m.email.to_string()).collect();
        assert_eq!(emails, vec!["m@x.it", "a@x.it", "b@x.it"]);
        assert_eq!(list[1].name.as_deref(), Some("Rossi, Anna"));
        assert!(parse_recipients("non-un-indirizzo").is_err());
    }

    fn account() -> Account {
        Account {
            id: "m@x.it".into(),
            email: "m@x.it".into(),
            name: "Marco".into(),
            imap_host: String::new(),
            imap_port: 993,
            smtp_host: String::new(),
            smtp_port: 465,
            signature_new: None,
            signature_reply: None,
            signature: String::new(),
            accept_invalid_certs: false,
        }
    }

    #[test]
    fn signature_sits_between_text_and_quote_with_inline_image() {
        // PNG di un pixel, come nelle firme vere
        let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
        let signature = format!("<table><tbody><tr><td><img src=\"{png}\" alt=\"Logo\"></td><td><div><b>Marco</b></div><div>Tel. 0331</div></td></tr></tbody></table>");
        let draft = Draft {
            to: "a@x.it",
            cc: "",
            bcc: "",
            subject: "R: Prova",
            body: "Le confermo.\n\n",
            quote: "Anna ha scritto:\n> Va bene?",
            signature: Some(&signature),
            files: Vec::new(),
            in_reply_to: None,
            references: None,
        };
        let raw = build(&account(), draft).unwrap().formatted();
        let msg = render::parse(&raw).unwrap();
        let html = msg.body_html(0).unwrap();
        let cid = html.split("src=\"cid:").nth(1).expect("immagine della firma con il suo cid");
        let cid = &cid[..cid.find('"').unwrap()];
        let image = msg.parts.iter().find(|p| p.content_id() == Some(cid)).expect("parte dell'immagine");
        assert_eq!(image.content_type().and_then(|c| c.subtype()), Some("png"));
        assert!(!html.contains("data:image"));
        assert!(html.find("Le confermo").unwrap() < html.find("<b>Marco</b>").unwrap());
        assert!(html.find("<b>Marco</b>").unwrap() < html.find("Anna ha scritto").unwrap());
        // versione testo: testo, firma, citazione
        assert_eq!(msg.body_text(0).unwrap().replace("\r\n", "\n"), "Le confermo.\n\nMarco\nTel. 0331\n\nAnna ha scritto:\n> Va bene?");
        // l'immagine della firma non è un allegato da elencare
        assert!(render::attachment_list(&raw).unwrap().is_empty());
    }

    #[test]
    fn builds_html_alternative_with_attachment() {
        let draft = Draft {
            to: "a@x.it",
            cc: "",
            bcc: "nascosto@x.it",
            subject: "Prova",
            body: "Ciao\n> citato https://x.it",
            quote: "",
            signature: None,
            files: vec![OutgoingFile { name: "nota.txt".into(), mime: "text/plain".into(), data: b"ok".to_vec() }],
            in_reply_to: Some("<abc@x.it>".into()),
            references: None,
        };
        let msg = build(&account(), draft).unwrap();
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

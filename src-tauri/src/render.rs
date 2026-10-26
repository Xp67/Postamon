//! Lettura dei messaggi (MIME) e preparazione dell'HTML da mostrare.
//!
//! L'HTML delle email è contenuto ostile: si ripulisce con ammonia (niente
//! script, niente form), le immagini da internet restano bloccate finché
//! l'utente non le chiede (sono il modo classico per sapere se e quando una
//! mail è stata letta) e le immagini allegate (`cid:`) si incorporano. Il
//! documento finale ha in più una Content-Security-Policy e l'interfaccia lo
//! mostra in un iframe senza permesso di eseguire script.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, anyhow};
use base64::Engine;
use mail_parser::{Address, Message, MessageParser, MessagePart, MimeHeaders};
use serde::Serialize;

use crate::store::HeaderRow;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub index: u32,
    pub name: String,
    pub mime: String,
    pub size: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rendered {
    pub subject: String,
    pub from_name: String,
    pub from_addr: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub date: i64,
    pub html: String,
    /// Il messaggio contiene immagini o stili da internet che non sono stati caricati.
    pub remote_blocked: bool,
    pub attachments: Vec<Attachment>,
}

pub fn parse(raw: &[u8]) -> Result<Message<'_>> {
    MessageParser::default().parse(raw).ok_or_else(|| anyhow!("Messaggio illeggibile"))
}

/// (nome, indirizzo) di ogni persona di un campo.
pub fn people(a: Option<&Address<'_>>) -> Vec<(String, String)> {
    a.map(|a| {
        a.iter()
            .map(|x| (x.name().unwrap_or("").trim().to_string(), x.address().unwrap_or("").trim().to_string()))
            .filter(|(_, addr)| !addr.is_empty())
            .collect()
    })
    .unwrap_or_default()
}

pub fn format_person((name, addr): &(String, String)) -> String {
    if name.is_empty() || name == addr { addr.clone() } else { format!("{name} <{addr}>") }
}

/// Riga d'archivio a partire dalle sole intestazioni.
pub fn header_row(header: &[u8]) -> HeaderRow {
    let Some(msg) = MessageParser::default().parse(header) else {
        return HeaderRow::default();
    };
    let (from_name, from_addr) = people(msg.from()).into_iter().next().unwrap_or_default();
    let has_attachments = msg.content_type().is_some_and(|ct| {
        ct.ctype().eq_ignore_ascii_case("application")
            || (ct.ctype().eq_ignore_ascii_case("multipart")
                && ct.subtype().is_some_and(|s| s.eq_ignore_ascii_case("mixed")))
    });
    HeaderRow {
        message_id: msg.message_id().unwrap_or("").to_string(),
        subject: msg.subject().unwrap_or("").trim().to_string(),
        from_name,
        from_addr,
        to: people(msg.to()).iter().map(format_person).collect::<Vec<_>>().join(", "),
        cc: people(msg.cc()).iter().map(format_person).collect::<Vec<_>>().join(", "),
        date: msg.date().map(|d| d.to_timestamp()).unwrap_or(0),
        has_attachments,
        ..Default::default()
    }
}

/// Anteprima del testo e presenza di allegati veri, a messaggio scaricato.
pub fn digest(raw: &[u8]) -> (String, bool) {
    let Ok(msg) = parse(raw) else {
        return (String::new(), false);
    };
    let preview = plain_text(&msg).split_whitespace().collect::<Vec<_>>().join(" ").chars().take(240).collect();
    let body = body_source(&msg);
    (preview, !attachments(&msg, &body).is_empty())
}

pub fn render(raw: &[u8], allow_remote: bool) -> Result<Rendered> {
    let msg = parse(raw)?;
    let (from_name, from_addr) = people(msg.from()).into_iter().next().unwrap_or_default();
    let body = body_source(&msg);
    let (clean, remote_blocked) = sanitize(&body, allow_remote, Arc::new(inline_images(&msg)));
    Ok(Rendered {
        subject: msg.subject().unwrap_or("").trim().to_string(),
        from_name,
        from_addr,
        to: people(msg.to()).iter().map(format_person).collect(),
        cc: people(msg.cc()).iter().map(format_person).collect(),
        date: msg.date().map(|d| d.to_timestamp()).unwrap_or(0),
        html: document(&clean, allow_remote),
        remote_blocked,
        attachments: attachments(&msg, &body),
    })
}

/// Testo del corpo: la parte di testo se c'è, altrimenti l'HTML convertito.
fn plain_text(msg: &Message<'_>) -> String {
    let text: Vec<&str> = msg
        .text_bodies()
        .filter(|p| p.is_text() && !p.is_text_html())
        .filter_map(|p| p.text_contents())
        .collect();
    if !text.is_empty() {
        return text.join("\n\n").replace("\r\n", "\n");
    }
    let html: Vec<&str> = msg.html_bodies().filter(|p| p.is_text_html()).filter_map(|p| p.text_contents()).collect();
    html_to_text(&html.join("\n"))
}

/// HTML → testo leggibile: titoli, paragrafi, righe di tabella e voci di
/// elenco vanno a capo; stili, script e intestazione si saltano.
pub fn html_to_text(html: &str) -> String {
    // to_ascii_lowercase non cambia la lunghezza: gli indici valgono per entrambe.
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len() / 2);
    let mut i = 0;
    while i < html.len() {
        let rest = &html[i..];
        if rest.starts_with("<!--") {
            match lower[i..].find("-->") {
                Some(p) => i += p + 3,
                None => break,
            }
            continue;
        }
        if rest.starts_with('<') {
            let Some(close) = rest.find('>') else { break };
            let tag = &lower[i + 1..i + close];
            let closing = tag.starts_with('/');
            let name: String = tag.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
            i += close + 1;
            match name.as_str() {
                "style" | "script" | "head" | "title" if !closing => {
                    let end = format!("</{name}");
                    match lower[i..].find(&end) {
                        Some(p) => {
                            i += p;
                            i += html[i..].find('>').map(|q| q + 1).unwrap_or(html.len() - i);
                        }
                        None => break,
                    }
                }
                "br" | "p" | "div" | "tr" | "table" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol"
                | "blockquote" | "section" | "article" | "header" | "footer" | "hr" | "center" => out.push('\n'),
                "li" if !closing => out.push_str("\n- "),
                "td" | "th" => out.push(' '),
                _ => {}
            }
            continue;
        }
        if rest.starts_with('&')
            && let Some(end) = rest[..rest.len().min(12)].find(';')
            && let Some(decoded) = entity(&rest[1..end])
        {
            out.push(decoded);
            i += end + 1;
            continue;
        }
        let c = rest.chars().next().unwrap_or(' ');
        out.push(if c.is_whitespace() { ' ' } else { c });
        i += c.len_utf8();
    }
    // Spazi compressi dentro le righe, al massimo una riga vuota di fila.
    let mut text = String::new();
    let mut blank = 0;
    for line in out.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            blank += 1;
            if blank > 1 || text.is_empty() {
                continue;
            }
        } else {
            blank = 0;
        }
        text.push_str(&line);
        text.push('\n');
    }
    text.trim_end().to_string()
}

fn entity(name: &str) -> Option<char> {
    if let Some(num) = name.strip_prefix('#') {
        let code = match num.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => num.parse().ok()?,
        };
        return char::from_u32(code).map(|c| if c == '\u{a0}' { ' ' } else { c });
    }
    Some(match name {
        "nbsp" => ' ',
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "agrave" => 'à',
        "egrave" => 'è',
        "eacute" => 'é',
        "igrave" => 'ì',
        "ograve" => 'ò',
        "ugrave" => 'ù',
        "Agrave" => 'À',
        "Egrave" => 'È',
        "Eacute" => 'É',
        "euro" => '€',
        "rsquo" | "lsquo" => '\'',
        "rdquo" | "ldquo" | "laquo" | "raquo" => '"',
        "ndash" => '–',
        "mdash" => '—',
        "hellip" => '…',
        "deg" => '°',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        _ => return None,
    })
}

/// HTML del corpo: la parte HTML se c'è, altrimenti il testo convertito.
fn body_source(msg: &Message<'_>) -> String {
    let html: Vec<&str> = msg.html_bodies().filter(|p| p.is_text_html()).filter_map(|p| p.text_contents()).collect();
    if !html.is_empty() {
        return html.join("\n");
    }
    let text: Vec<&str> = msg.text_bodies().filter(|p| p.is_text()).filter_map(|p| p.text_contents()).collect();
    format!("<div class=\"pm-plain\">{}</div>", linkify(&text.join("\n\n")))
}

/// Immagini con Content-ID, pronte da incorporare come `data:`.
fn inline_images(msg: &Message<'_>) -> HashMap<String, String> {
    msg.parts
        .iter()
        .filter_map(|p| {
            let cid = p.content_id()?.trim_matches(['<', '>']).to_string();
            let mime = mime_of(p);
            mime.starts_with("image/").then(|| {
                let data = base64::engine::general_purpose::STANDARD.encode(p.contents());
                (cid, format!("data:{mime};base64,{data}"))
            })
        })
        .collect()
}

fn attachments(msg: &Message<'_>, body: &str) -> Vec<Attachment> {
    msg.attachments()
        .enumerate()
        .filter(|(_, p)| {
            // Le immagini già mostrate dentro il testo non sono allegati da elencare.
            p.content_id()
                .map(|cid| !body.contains(&format!("cid:{}", cid.trim_matches(['<', '>']))))
                .unwrap_or(true)
        })
        .map(|(i, p)| Attachment { index: i as u32, name: attachment_name(p, i), mime: mime_of(p), size: p.len() })
        .collect()
}

pub fn attachment_name(p: &MessagePart<'_>, index: usize) -> String {
    if let Some(name) = p.attachment_name().map(str::trim).filter(|n| !n.is_empty()) {
        return name.to_string();
    }
    if let Some(subject) = p.message().and_then(|m| m.subject()) {
        return format!("{}.eml", subject.trim());
    }
    let ext = mime_guess::get_mime_extensions_str(&mime_of(p))
        .and_then(|e| e.first())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();
    format!("allegato-{}{ext}", index + 1)
}

fn mime_of(p: &MessagePart<'_>) -> String {
    p.content_type()
        .map(|ct| match ct.subtype() {
            Some(sub) => format!("{}/{}", ct.ctype(), sub),
            None => ct.ctype().to_string(),
        })
        .unwrap_or_else(|| "application/octet-stream".to_string())
        .to_lowercase()
}

/// Nome e contenuto di un allegato, per salvarlo o inoltrarlo.
pub fn attachment_data(raw: &[u8], index: u32) -> Result<(String, String, Vec<u8>)> {
    let msg = parse(raw)?;
    let part = msg.attachment(index).ok_or_else(|| anyhow!("Allegato non trovato"))?;
    Ok((attachment_name(part, index as usize), mime_of(part), part.contents().to_vec()))
}

pub fn attachment_list(raw: &[u8]) -> Result<Vec<Attachment>> {
    let msg = parse(raw)?;
    let body = body_source(&msg);
    Ok(attachments(&msg, &body))
}

pub fn all_attachments(raw: &[u8]) -> Result<Vec<(String, String, Vec<u8>)>> {
    let msg = parse(raw)?;
    let body = body_source(&msg);
    attachments(&msg, &body)
        .iter()
        .map(|a| attachment_data(raw, a.index))
        .collect()
}

// ---- risposta e inoltro ----

pub struct Original {
    pub subject: String,
    pub from: (String, String),
    pub reply_to: Vec<(String, String)>,
    pub to: Vec<(String, String)>,
    pub cc: Vec<(String, String)>,
    pub date: String,
    pub text: String,
    pub message_id: Option<String>,
    pub references: Vec<String>,
}

pub fn original(raw: &[u8]) -> Result<Original> {
    let msg = parse(raw)?;
    let mut references: Vec<String> = match msg.references().as_text_list() {
        Some(list) => list.iter().map(|s| s.to_string()).collect(),
        None => msg.references().as_text().map(|s| vec![s.to_string()]).unwrap_or_default(),
    };
    references.retain(|r| !r.is_empty());
    Ok(Original {
        subject: msg.subject().unwrap_or("").trim().to_string(),
        from: people(msg.from()).into_iter().next().unwrap_or_default(),
        reply_to: people(msg.reply_to()),
        to: people(msg.to()),
        cc: people(msg.cc()),
        date: msg.date().map(italian_date).unwrap_or_default(),
        text: plain_text(&msg),
        message_id: msg.message_id().map(str::to_string),
        references,
    })
}

fn italian_date(d: &mail_parser::DateTime) -> String {
    const MONTHS: [&str; 12] = [
        "gennaio", "febbraio", "marzo", "aprile", "maggio", "giugno", "luglio", "agosto", "settembre", "ottobre",
        "novembre", "dicembre",
    ];
    let month = MONTHS.get((d.month as usize).wrapping_sub(1)).copied().unwrap_or("");
    format!("{} {} {}, {:02}:{:02}", d.day, month, d.year, d.hour, d.minute)
}

// ---- HTML ----

fn sanitize(html: &str, allow_remote: bool, cids: Arc<HashMap<String, String>>) -> (String, bool) {
    let blocked = Arc::new(AtomicBool::new(false));
    let flag = blocked.clone();
    let mut b = ammonia::Builder::default();
    b.rm_clean_content_tags(&["style"])
        .add_clean_content_tags(&["title"])
        .add_tags(&["style", "font", "center", "big"])
        .add_generic_attributes(&[
            "style",
            "class",
            "align",
            "valign",
            "bgcolor",
            "background",
            "width",
            "height",
            "border",
            "cellpadding",
            "cellspacing",
            "color",
            "face",
            "size",
            "dir",
            "lang",
            "title",
        ])
        .add_url_schemes(&["cid", "data"])
        .link_rel(Some("noopener noreferrer"))
        .attribute_filter(move |_element, attribute, value| match attribute {
            "src" | "background" => {
                let v = value.trim();
                if let Some(id) = v.strip_prefix("cid:") {
                    return cids.get(id.trim_matches(['<', '>'])).map(|d| Cow::Owned(d.clone()));
                }
                if v.starts_with("data:image/") {
                    return Some(Cow::Borrowed(value));
                }
                if is_remote(v) {
                    if allow_remote {
                        return Some(Cow::Borrowed(value));
                    }
                    flag.store(true, Ordering::Relaxed);
                }
                None
            }
            "style" => {
                if !allow_remote && has_remote_url(value) {
                    flag.store(true, Ordering::Relaxed);
                }
                Some(Cow::Borrowed(value))
            }
            _ => Some(Cow::Borrowed(value)),
        });
    let clean = b.clean(html).to_string();
    // Gli sfondi dentro i blocchi <style> li ferma la Content-Security-Policy:
    // qui serve solo per dire all'utente che c'è qualcosa di bloccato.
    if !allow_remote && has_remote_url(html) {
        blocked.store(true, Ordering::Relaxed);
    }
    (clean, blocked.load(Ordering::Relaxed))
}

fn is_remote(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("//")
}

fn has_remote_url(css: &str) -> bool {
    let lower = css.to_ascii_lowercase();
    lower.match_indices("url(").any(|(i, _)| {
        let rest = lower[i + 4..].trim_start_matches([' ', '"', '\'']);
        is_remote(rest)
    })
}

const BASE_CSS: &str = "html{color-scheme:light}\
body{margin:0;padding:20px 24px;font:14px/1.55 'Segoe UI',system-ui,sans-serif;color:#18212b;background:#fff}\
img{max-width:100%;height:auto}\
a{color:#0f6f8f}\
.pm-plain{white-space:pre-wrap;overflow-wrap:anywhere}\
blockquote{margin:0 0 0 4px;padding-left:12px;border-left:3px solid #d5dde4;color:#4a5866}";

fn document(body: &str, allow_remote: bool) -> String {
    let img = if allow_remote { "data: https: http:" } else { "data:" };
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; \
         img-src {img}; font-src data:; media-src data:\">\
         <style>{BASE_CSS}</style></head><body>{body}</body></html>"
    )
}

pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// Testo semplice → HTML con i link cliccabili.
pub fn linkify(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    loop {
        let next = ["http://", "https://"].iter().filter_map(|p| rest.find(p)).min();
        let Some(start) = next else {
            out.push_str(&escape(rest));
            return out;
        };
        out.push_str(&escape(&rest[..start]));
        let tail = &rest[start..];
        let mut end = tail.find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"')).unwrap_or(tail.len());
        while end > 0 && tail[..end].ends_with(['.', ',', ';', ':', '!', '?', ')', ']', '\'']) {
            end -= 1;
        }
        let url = &tail[..end];
        out.push_str(&format!("<a href=\"{0}\">{0}</a>", escape(url)));
        rest = &tail[end..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HTML_MAIL: &str = "From: Mario Rossi <mario@esempio.it>\r\n\
To: Marco <marco@dimensionesalotto.it>\r\n\
Subject: =?utf-8?Q?Offerta_pi=C3=B9_bella?=\r\n\
Date: Wed, 21 Oct 2026 10:42:00 +0200\r\n\
Message-ID: <abc@esempio.it>\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/related; boundary=\"b1\"\r\n\
\r\n\
--b1\r\n\
Content-Type: text/html; charset=utf-8\r\n\
\r\n\
<html><head><title>Titolo</title><style>.x{color:red}</style></head><body>\
<script>alert(1)</script><p class=\"x\" onclick=\"evil()\">Ciao</p>\
<img src=\"https://tracker.example/pixel.gif\"><img src=\"cid:logo@x\">\
<form action=\"https://phish\"><input name=p></form></body></html>\r\n\
--b1\r\n\
Content-Type: image/png\r\n\
Content-ID: <logo@x>\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
iVBORw0KGgo=\r\n\
--b1--\r\n";

    #[test]
    fn sanitizes_and_blocks_remote_images() {
        let r = render(HTML_MAIL.as_bytes(), false).unwrap();
        assert_eq!(r.subject, "Offerta più bella");
        assert_eq!(r.from_name, "Mario Rossi");
        assert!(r.remote_blocked);
        assert!(!r.html.contains("<script"));
        assert!(!r.html.contains("onclick"));
        assert!(!r.html.contains("tracker.example"));
        assert!(!r.html.contains("<form"));
        assert!(!r.html.contains("Titolo"));
        assert!(r.html.contains("class=\"x\""));
        assert!(r.html.contains(".x{color:red}"));
        assert!(r.html.contains("data:image/png;base64,iVBORw0KGgo="));
        // il logo è mostrato nel testo, quindi non compare fra gli allegati
        assert!(r.attachments.is_empty());
    }

    #[test]
    fn remote_images_load_when_allowed() {
        let r = render(HTML_MAIL.as_bytes(), true).unwrap();
        assert!(!r.remote_blocked);
        assert!(r.html.contains("https://tracker.example/pixel.gif"));
        assert!(r.html.contains("img-src data: https: http:"));
    }

    #[test]
    fn plain_text_is_escaped_and_linked() {
        let raw = "From: a@b.it\r\nSubject: t\r\nContent-Type: text/plain; charset=iso-8859-1\r\n\
Content-Transfer-Encoding: quoted-printable\r\n\r\nPrezzo <b>10</b> =E0 vedi https://dimensionesalotto.it/promo.\r\n";
        let r = render(raw.as_bytes(), false).unwrap();
        assert!(r.html.contains("Prezzo &lt;b&gt;10&lt;/b&gt; à vedi"));
        assert!(r.html.contains("<a href=\"https://dimensionesalotto.it/promo\""));
        assert!(r.html.contains("</a>."));
    }

    #[test]
    fn preview_keeps_blocks_apart() {
        let raw = "From: a@b.it\r\nSubject: t\r\nContent-Type: text/html; charset=utf-8\r\n\r\n\
<h1>Collezione Autunno</h1><p>Velluti e   bouclé</p><style>.x{}</style>\r\n";
        let (preview, has_attachments) = digest(raw.as_bytes());
        assert_eq!(preview, "Collezione Autunno Velluti e bouclé");
        assert!(!has_attachments);
    }

    #[test]
    fn html_to_text_for_quotes() {
        let html = "<html><head><title>T</title><style>p{}</style></head><body><!-- c -->\
<p>Gentile cliente,</p><p>l&#39;ordine <b>2026/1187</b> &egrave; partito.</p>\
<table><tr><td>Poltrona</td><td>2</td></tr></table><ul><li>uno</li><li>due</li></ul>Grazie&nbsp;&amp; saluti</body></html>";
        assert_eq!(
            html_to_text(html),
            "Gentile cliente,\n\nl'ordine 2026/1187 è partito.\n\nPoltrona 2\n\n- uno\n- due\nGrazie & saluti"
        );
    }

    #[test]
    fn lists_real_attachments() {
        let raw = "From: a@b.it\r\nSubject: t\r\nContent-Type: multipart/mixed; boundary=\"z\"\r\n\r\n\
--z\r\nContent-Type: text/plain\r\n\r\nciao\r\n\
--z\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=\"preventivo.pdf\"\r\n\
Content-Transfer-Encoding: base64\r\n\r\nJVBERi0=\r\n--z--\r\n";
        let r = render(raw.as_bytes(), false).unwrap();
        assert_eq!(r.attachments.len(), 1);
        assert_eq!(r.attachments[0].name, "preventivo.pdf");
        assert_eq!(r.attachments[0].mime, "application/pdf");
        let (name, _, data) = attachment_data(raw.as_bytes(), 0).unwrap();
        assert_eq!((name.as_str(), data.as_slice()), ("preventivo.pdf", b"%PDF-".as_slice()));
        assert!(header_row(raw.as_bytes()).has_attachments);
    }
}

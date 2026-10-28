//! Firme: un elenco con nome, come in Outlook, salvato in `signatures.json`.
//!
//! Ogni firma è HTML già ripulito (stesso trattamento della posta in arrivo,
//! ma senza `class`: una firma non ha fogli di stile) con le immagini
//! incorporate come `data:`, così editor e finestra di scrittura le mostrano
//! senza file a parte. All'invio `for_mail` le trasforma in parti `cid:` del
//! messaggio. Per ogni casella si sceglie la firma dei messaggi nuovi e quella
//! di risposte e inoltri (`Account::signature_new` e `signature_reply`).
//!
//! Alla prima partenza l'elenco nasce dalle firme di Outlook (`outlook`), con
//! le immagini della loro cartella `_file`.

use std::borrow::Cow;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result, bail};
use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::accounts::{self, Accounts};
use crate::render;

/// Oltre questa misura un'immagine non è da firma: va ridotta prima.
const MAX_IMAGE: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Signature {
    pub id: String,
    pub name: String,
    pub html: String,
}

pub struct Signatures {
    path: PathBuf,
    list: Mutex<Vec<Signature>>,
}

impl Signatures {
    /// Il secondo valore dice se il file non c'era ancora: primo avvio con
    /// l'elenco delle firme, da riempire con quelle di Outlook.
    pub fn load(path: PathBuf) -> Result<(Self, bool)> {
        let (list, fresh) = match std::fs::read(&path) {
            Ok(bytes) => (serde_json::from_slice(&bytes).context("lettura di signatures.json")?, false),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Vec::new(), true),
            Err(e) => return Err(e).context("lettura di signatures.json"),
        };
        Ok((Self { path, list: Mutex::new(list) }, fresh))
    }

    pub fn all(&self) -> Vec<Signature> {
        self.lock().clone()
    }

    pub fn get(&self, id: &str) -> Option<Signature> {
        self.lock().iter().find(|s| s.id == id).cloned()
    }

    /// Crea (senza `id`) o aggiorna una firma. L'HTML arriva dall'editor: si
    /// ripulisce qui, e l'interfaccia mostra solo quello che torna indietro.
    pub fn save(&self, id: Option<&str>, name: &str, html: &str) -> Result<Signature> {
        let name = name.trim();
        if name.is_empty() {
            bail!("Dai un nome alla firma");
        }
        let sig = Signature {
            id: id.map(str::to_string).unwrap_or_else(new_id),
            name: name.to_string(),
            html: clean(html),
        };
        let mut list = self.lock();
        match list.iter_mut().find(|s| s.id == sig.id) {
            Some(slot) => *slot = sig.clone(),
            None if id.is_some() => bail!("La firma non c'è più: è stata eliminata"),
            None => list.push(sig.clone()),
        }
        write(&self.path, &list)?;
        Ok(sig)
    }

    /// Aggiunge firme con HTML già pulito (import da Outlook, firme di testo
    /// delle versioni precedenti). Restituisce quelle create, nello stesso ordine.
    pub fn add(&self, items: Vec<(String, String)>) -> Result<Vec<Signature>> {
        let created: Vec<Signature> =
            items.into_iter().map(|(name, html)| Signature { id: new_id(), name, html }).collect();
        let mut list = self.lock();
        list.extend(created.iter().cloned());
        write(&self.path, &list)?;
        Ok(created)
    }

    pub fn remove(&self, id: &str) -> Result<()> {
        let mut list = self.lock();
        list.retain(|s| s.id != id);
        write(&self.path, &list)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Signature>> {
        self.list.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// Primo avvio con l'elenco delle firme: si importano quelle di Outlook e le
/// firme di testo delle versioni precedenti, e ogni casella prende la sua.
/// La firma di testo copiata da Outlook alla configurazione era la sua
/// versione .txt: se l'utente non l'ha cambiata, vale quella vera, con le
/// immagini. Vale anche per gli id che non esistono più (Postamon Dev parte
/// con le caselle dell'installato, ma non con le sue firme).
pub fn first_run(sigs: &Signatures, accounts: &Accounts) -> Result<()> {
    let imported = sigs.add(outlook().into_iter().map(|o| (o.name, o.html)).collect())?;
    for mut acc in accounts.all() {
        let legacy = std::mem::take(&mut acc.signature);
        let legacy = legacy.trim();
        let custom = !legacy.is_empty() && accounts::outlook_signature(&acc.email).as_deref() != Some(legacy);
        let chosen = if custom {
            sigs.add(vec![(format!("Firma di {}", acc.email), from_text(legacy))])?.pop().map(|s| s.id)
        } else {
            imported.iter().find(|s| outlook_email(&s.name).as_deref() == Some(acc.id.as_str())).map(|s| s.id.clone())
        };
        for slot in [&mut acc.signature_new, &mut acc.signature_reply] {
            if slot.as_ref().is_none_or(|id| sigs.get(id).is_none()) {
                *slot = chosen.clone();
            }
        }
        accounts.upsert(acc)?;
    }
    Ok(())
}

/// Firma di una casella appena aggiunta: quella di Outlook per il suo
/// indirizzo, già nell'elenco o importata adesso.
pub fn for_new_account(sigs: &Signatures, email: &str) -> Option<String> {
    let id = accounts::account_id(email);
    let mine = |name: &str| outlook_email(name).as_deref() == Some(id.as_str());
    if let Some(s) = sigs.all().into_iter().find(|s| mine(&s.name)) {
        return Some(s.id);
    }
    let o = outlook().into_iter().find(|o| mine(&o.name))?;
    sigs.add(vec![(o.name, o.html)]).ok()?.pop().map(|s| s.id)
}

fn write(path: &Path, list: &[Signature]) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(list)?).context("salvataggio delle firme")?;
    std::fs::rename(&tmp, path).context("salvataggio delle firme")?;
    Ok(())
}

fn new_id() -> String {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("generatore casuale del sistema");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ---- HTML ----

/// HTML di una firma ripulito: niente script, eventi, moduli né fogli di
/// stile; restano formattazione in linea, tabelle, link e immagini.
pub fn clean(html: &str) -> String {
    trim_blank_edges(&builder(None).clean(html).to_string())
}

/// `images`: cartella da cui leggere le immagini con un percorso relativo
/// (quella delle firme di Outlook). Senza, quelle immagini si tolgono.
fn builder(images: Option<PathBuf>) -> ammonia::Builder<'static> {
    let mut b = ammonia::Builder::default();
    b.add_clean_content_tags(&["title"])
        .add_tags(&["font", "center"])
        .add_generic_attributes(&[
            "style",
            "align",
            "valign",
            "bgcolor",
            "width",
            "height",
            "border",
            "cellpadding",
            "cellspacing",
            "color",
            "face",
            "size",
            "dir",
            "title",
        ])
        .add_url_schemes(&["data"])
        .link_rel(None)
        .attribute_filter(move |_element, attribute, value| match attribute {
            "src" | "background" => image_src(value, images.as_deref()),
            "href" => link(value),
            "style" => style(value).map(Cow::Owned),
            _ => Some(Cow::Borrowed(value)),
        });
    b
}

fn image_src<'u>(value: &'u str, dir: Option<&Path>) -> Option<Cow<'u, str>> {
    let v = value.trim();
    let lower = v.to_ascii_lowercase();
    if lower.starts_with("data:image/") || lower.starts_with("https://") || lower.starts_with("http://") {
        return Some(Cow::Borrowed(value));
    }
    if lower.starts_with("data:") || lower.contains(':') {
        return None;
    }
    let dir = dir?;
    let file = ammonia::url::Url::from_directory_path(dir).ok()?.join(v).ok()?.to_file_path().ok()?;
    if !file.starts_with(dir) {
        return None;
    }
    image(&file).ok().map(Cow::Owned)
}

/// I filtri antispam riscrivono i link delle mail ricevute: in una firma
/// copiata da lì si rimette l'indirizzo vero.
fn link(value: &str) -> Option<Cow<'_, str>> {
    let lower = value.trim().to_ascii_lowercase();
    if lower.starts_with("data:") {
        return None;
    }
    let Ok(url) = ammonia::url::Url::parse(value.trim()) else {
        return Some(Cow::Borrowed(value));
    };
    let param = match url.host_str() {
        Some("urlsand.esvalabs.com") => "u",
        Some(h) if h.ends_with(".safelinks.protection.outlook.com") => "url",
        _ => return Some(Cow::Borrowed(value)),
    };
    let real = url.query_pairs().find(|(k, _)| k == param).map(|(_, v)| v.into_owned());
    match real {
        Some(r) if ["http://", "https://", "mailto:", "tel:"].iter().any(|p| r.to_ascii_lowercase().starts_with(p)) => {
            Some(Cow::Owned(r))
        }
        _ => Some(Cow::Borrowed(value)),
    }
}

/// Stile in linea senza le proprietà di Word (`mso-*` e simili), che i
/// browser ignorano e che rendono illeggibile l'HTML.
fn style(value: &str) -> Option<String> {
    let kept: Vec<String> = value
        .split(';')
        .map(|d| d.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|d| {
            let prop = d.split(':').next().unwrap_or("").trim().to_ascii_lowercase();
            !d.is_empty()
                && d.contains(':')
                && !prop.starts_with("mso-")
                && !["text-underline", "tab-interval", "tab-stops", "layout-grid-mode"].contains(&prop.as_str())
        })
        .collect();
    (!kept.is_empty()).then(|| kept.join(";"))
}

/// Immagine da un file, come `data:`. Solo i formati che si vedono in tutti
/// i programmi di posta.
pub fn image(path: &Path) -> Result<String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mime = match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("svg") => bail!("{name}: le immagini SVG non si vedono in molti programmi di posta, usa PNG o JPG"),
        _ => bail!("{name}: per la firma servono immagini PNG, JPG o GIF"),
    };
    let data = std::fs::read(path).with_context(|| format!("Lettura di {name} non riuscita"))?;
    if data.len() > MAX_IMAGE {
        bail!("{name} è troppo grande per una firma (massimo 2 MB): riducila prima di inserirla");
    }
    Ok(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(&data)))
}

/// Firma di testo delle versioni precedenti → HTML, riga per riga.
pub fn from_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .trim()
        .lines()
        .map(|l| {
            if l.trim().is_empty() { "<div><br></div>".to_string() } else { format!("<div>{}</div>", render::linkify(l)) }
        })
        .collect()
}

/// Versione testo della firma, per la parte `text/plain` del messaggio. Non
/// basta `html_to_text`: le firme di Word annidano righe di tabella e
/// paragrafi, e andando a capo per ognuno resterebbe una riga vuota fra un
/// recapito e l'altro. Qui un blocco va a capo una volta sola; le righe vuote
/// sono solo quelle volute (un blocco senza testo, un `<br>`).
pub fn to_text(html: &str) -> String {
    let mut t = TextOut::default();
    for n in &parse(html) {
        t.node(n, html);
    }
    t.soft_break();
    let mut text = String::new();
    let mut blank = 0;
    for line in t.lines.iter().map(|l| l.trim()) {
        blank = if line.is_empty() { blank + 1 } else { 0 };
        if blank > 1 || (blank == 1 && text.is_empty()) {
            continue;
        }
        text.push_str(line);
        text.push('\n');
    }
    text.trim_end().to_string()
}

const BLOCKS: &[&str] = &[
    "div", "p", "table", "tr", "ul", "ol", "li", "h1", "h2", "h3", "h4", "h5", "h6", "blockquote", "center", "pre",
];

#[derive(Default)]
struct TextOut {
    lines: Vec<String>,
    line: String,
}

impl TextOut {
    /// A capo, se la riga ha già qualcosa.
    fn soft_break(&mut self) {
        if !self.line.trim().is_empty() {
            self.lines.push(std::mem::take(&mut self.line));
        }
        self.line.clear();
    }

    fn push(&mut self, text: &str) {
        for (i, word) in text.split(' ').enumerate() {
            if i > 0 && !self.line.is_empty() && !self.line.ends_with(' ') {
                self.line.push(' ');
            }
            self.line.push_str(word);
        }
    }

    fn node(&mut self, node: &Node, html: &str) {
        match node {
            Node::Text(r) => {
                let raw = html[r.clone()].replace("&nbsp;", " ");
                let inner = render::html_to_text(&raw);
                let lead = raw.starts_with(char::is_whitespace);
                let trail = raw.ends_with(char::is_whitespace);
                self.push(&format!("{}{inner}{}", if lead { " " } else { "" }, if trail && !inner.is_empty() { " " } else { "" }));
            }
            Node::Void(name, _) if name == "br" => self.lines.push(std::mem::take(&mut self.line)),
            Node::Void(name, _) if name == "hr" => {
                self.soft_break();
                self.lines.push("—————".to_string());
            }
            Node::Void(..) => {}
            Node::Element { name, open, children, .. } => {
                let block = BLOCKS.contains(&name.as_str());
                if block {
                    self.soft_break();
                    // un blocco senza testo né immagini è una riga vuota voluta
                    if !STRUCTURE.contains(&name.as_str()) && blank(node, html) {
                        self.lines.push(String::new());
                        return;
                    }
                }
                if name == "li" {
                    self.line.push_str("- ");
                }
                let (lines_before, column) = (self.lines.len(), self.line.len());
                for c in children {
                    self.node(c, html);
                }
                match name.as_str() {
                    "td" | "th" => self.push(" "),
                    "a" => {
                        // indirizzo del link se il testo non lo dice già
                        let href = attribute(&html[open.clone()], "href").unwrap_or_default();
                        let target = href.trim_start_matches("mailto:").trim_start_matches("tel:");
                        let from = if self.lines.len() == lines_before { column } else { 0 };
                        let shown = self.line.get(from..).unwrap_or("").trim().to_string();
                        if !target.is_empty() && !shown.is_empty() && !target.contains(&shown) && !shown.contains(target) {
                            self.push(&format!(" <{target}>"));
                        }
                    }
                    _ => {}
                }
                if block {
                    self.soft_break();
                }
            }
        }
    }
}

/// Valore di un attributo nel tag di apertura serializzato da ammonia.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let start = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    let end = start + tag[start..].find('"')?;
    Some(tag[start..end].replace("&amp;", "&").replace("&quot;", "\""))
}

/// Immagine della firma da spedire come parte del messaggio.
pub struct Inline {
    pub cid: String,
    pub mime: String,
    pub data: Vec<u8>,
}

/// HTML della firma per un messaggio: le immagini `data:` diventano `cid:`
/// (molti programmi di posta non mostrano i `data:`), una parte sola per
/// un'immagine che compare più volte.
pub fn for_mail(html: &str) -> (String, Vec<Inline>) {
    let tag = new_id();
    let mut out = String::with_capacity(html.len());
    let mut seen: Vec<&str> = Vec::new();
    let mut images: Vec<Inline> = Vec::new();
    let mut rest = html;
    while let Some(p) = rest.find("src=\"data:") {
        let start = p + "src=\"".len();
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let end = tail.find('"').unwrap_or(tail.len());
        let uri = &tail[..end];
        let index = match seen.iter().position(|u| *u == uri) {
            Some(i) => Some(i),
            None => decode_data_uri(uri).map(|(mime, data)| {
                seen.push(uri);
                images.push(Inline { cid: format!("firma{}.{tag}@postamon", images.len() + 1), mime, data });
                images.len() - 1
            }),
        };
        match index {
            Some(i) => {
                out.push_str("cid:");
                out.push_str(&images[i].cid);
            }
            None => out.push_str(uri),
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    (out, images)
}

fn decode_data_uri(uri: &str) -> Option<(String, Vec<u8>)> {
    let (head, data) = uri.strip_prefix("data:")?.split_once(',')?;
    let mime = head.strip_suffix(";base64")?;
    if !mime.starts_with("image/") {
        return None;
    }
    let data: String = data.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = base64::engine::general_purpose::STANDARD.decode(data).ok()?;
    Some((mime.to_string(), bytes))
}

// ---- righe vuote in testa e in coda ----

/// Pezzo dell'HTML già pulito da ammonia, che è ben formato: tag chiusi in
/// ordine, attributi fra virgolette doppie.
enum Node {
    Text(Range<usize>),
    /// `<br>`, `<img>`, `<hr>`…: niente da chiudere.
    Void(String, Range<usize>),
    Element { name: String, open: Range<usize>, close: Option<Range<usize>>, children: Vec<Node> },
}

const VOID: &[&str] = &["br", "img", "hr", "wbr", "col", "area", "input", "source"];
/// Struttura delle tabelle: anche vuota tiene il suo posto, non si toglie mai.
const STRUCTURE: &[&str] = &["table", "thead", "tbody", "tfoot", "tr", "td", "th", "colgroup"];

/// Toglie spazi e righe vuote in testa e in coda (Word ne lascia sempre un
/// paio sotto la firma); quelli in mezzo restano com'erano.
fn trim_blank_edges(html: &str) -> String {
    let mut nodes = parse(html);
    trim_start(&mut nodes, html);
    trim_end(&mut nodes, html);
    let mut out = String::with_capacity(html.len());
    for n in &nodes {
        serialize(n, html, &mut out);
    }
    out
}

fn parse(html: &str) -> Vec<Node> {
    // (nome, apertura, figli) degli elementi ancora aperti; il primo è la radice.
    let mut stack: Vec<(String, Range<usize>, Vec<Node>)> = vec![(String::new(), 0..0, Vec::new())];
    let bytes = html.as_bytes();
    let mut i = 0;
    let mut text_start = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        // fine del tag, saltando i '>' dentro le virgolette
        let mut j = i + 1;
        let mut quoted = false;
        while j < bytes.len() && (quoted || bytes[j] != b'>') {
            if bytes[j] == b'"' {
                quoted = !quoted;
            }
            j += 1;
        }
        let end = (j + 1).min(bytes.len());
        let inner = &html[i + 1..j.min(bytes.len())];
        let closing = inner.starts_with('/');
        let name: String =
            inner.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
        if name.is_empty() {
            i = end;
            continue;
        }
        if text_start < i {
            stack.last_mut().unwrap().2.push(Node::Text(text_start..i));
        }
        if closing {
            if let Some(pos) = stack.iter().rposition(|(n, _, _)| *n == name).filter(|&p| p > 0) {
                while stack.len() > pos + 1 {
                    let (n, open, children) = stack.pop().unwrap();
                    stack.last_mut().unwrap().2.push(Node::Element { name: n, open, close: None, children });
                }
                let (n, open, children) = stack.pop().unwrap();
                stack.last_mut().unwrap().2.push(Node::Element { name: n, open, close: Some(i..end), children });
            }
        } else if VOID.contains(&name.as_str()) {
            stack.last_mut().unwrap().2.push(Node::Void(name, i..end));
        } else {
            stack.push((name, i..end, Vec::new()));
        }
        i = end;
        text_start = end;
    }
    if text_start < html.len() {
        stack.last_mut().unwrap().2.push(Node::Text(text_start..html.len()));
    }
    while stack.len() > 1 {
        let (n, open, children) = stack.pop().unwrap();
        stack.last_mut().unwrap().2.push(Node::Element { name: n, open, close: None, children });
    }
    stack.pop().map(|(_, _, children)| children).unwrap_or_default()
}

fn blank(node: &Node, html: &str) -> bool {
    match node {
        Node::Text(r) => html[r.clone()].replace("&nbsp;", " ").trim().is_empty(),
        Node::Void(name, _) => name == "br",
        Node::Element { name, children, .. } => {
            !STRUCTURE.contains(&name.as_str()) && children.iter().all(|c| blank(c, html))
        }
    }
}

fn trim_end(nodes: &mut Vec<Node>, html: &str) {
    while let Some(last) = nodes.last_mut() {
        if blank(last, html) {
            nodes.pop();
            continue;
        }
        if let Node::Element { children, .. } = last {
            trim_end(children, html);
        }
        break;
    }
}

fn trim_start(nodes: &mut Vec<Node>, html: &str) {
    while let Some(first) = nodes.first_mut() {
        if blank(first, html) {
            nodes.remove(0);
            continue;
        }
        if let Node::Element { children, .. } = first {
            trim_start(children, html);
        }
        break;
    }
}

fn serialize(node: &Node, html: &str, out: &mut String) {
    match node {
        // Immagine rimasta senza sorgente dopo la pulizia (file che non c'è,
        // `src` non ammesso): si vedrebbe solo l'icona dell'immagine rotta.
        Node::Void(name, r) if name == "img" && attribute(&html[r.clone()], "src").is_none() => {}
        Node::Text(r) | Node::Void(_, r) => out.push_str(&html[r.clone()]),
        Node::Element { open, close, children, .. } => {
            out.push_str(&html[open.clone()]);
            for c in children {
                serialize(c, html, out);
            }
            if let Some(c) = close {
                out.push_str(&html[c.clone()]);
            }
        }
    }
}

// ---- Outlook ----

pub struct Outlook {
    pub name: String,
    pub html: String,
}

/// Firme di Outlook di questo PC (`%APPDATA%\Microsoft\Signatures\*.htm`),
/// già pulite e con le immagini incorporate, in ordine di nome.
pub fn outlook() -> Vec<Outlook> {
    match std::env::var_os("APPDATA") {
        Some(appdata) => outlook_in(&PathBuf::from(appdata).join("Microsoft").join("Signatures")),
        None => Vec::new(),
    }
}

fn outlook_in(dir: &Path) -> Vec<Outlook> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut list: Vec<Outlook> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("htm")))
        .filter_map(|p| {
            let name = p.file_stem()?.to_str()?.to_string();
            let html = outlook_html(dir, &std::fs::read(&p).ok()?);
            (!html.is_empty()).then_some(Outlook { name, html })
        })
        .collect();
    list.sort_by_key(|o| o.name.to_lowercase());
    list
}

/// Indirizzo nel nome di una firma di Outlook, come "Marco (marco@dominio.it)":
/// è la firma che Outlook usa per quella casella.
pub fn outlook_email(name: &str) -> Option<String> {
    let inner = name.rsplit_once('(')?.1.strip_suffix(')')?;
    inner.contains('@').then(|| inner.trim().to_lowercase())
}

fn outlook_html(dir: &Path, bytes: &[u8]) -> String {
    let source = decode_html(bytes);
    // to_ascii_lowercase non cambia la lunghezza: gli indici valgono per entrambe.
    let lower = source.to_ascii_lowercase();
    let body = match lower.find("<body") {
        Some(start) => {
            let open = lower[start..].find('>').map_or(source.len(), |p| start + p + 1);
            let end = lower.rfind("</body").filter(|&e| e >= open).unwrap_or(source.len());
            &source[open..end]
        }
        None => &source[..],
    };
    let clean = trim_blank_edges(&builder(Some(dir.to_path_buf())).clean(&paragraphs_to_divs(body)).to_string());
    if clean.is_empty() {
        return clean;
    }
    match base_font(&source, &lower) {
        Some(style) => format!("<div style=\"{style}\">{clean}</div>"),
        None => clean,
    }
}

/// In Word `<p class=MsoNormal>` ha margine zero, ma lo dice il foglio di
/// stile dell'intestazione, che non si porta dietro: con `<div>` il risultato
/// è lo stesso in ogni programma di posta.
fn paragraphs_to_divs(html: &str) -> String {
    let mut out = String::with_capacity(html.len() + html.len() / 20);
    let bytes = html.as_bytes();
    let mut last = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let (skip, with) = match bytes.get(i + 1..i + 3) {
                Some([b'p' | b'P', c]) if matches!(c, b' ' | b'>' | b'\n' | b'\r' | b'\t') => (2, "<div"),
                Some([b'/', b'p' | b'P']) if matches!(bytes.get(i + 3), Some(b'>' | b' ' | b'\n' | b'\r')) => {
                    (3, "</div")
                }
                _ => (0, ""),
            };
            if skip > 0 {
                out.push_str(&html[last..i]);
                out.push_str(with);
                i += skip;
                last = i;
                continue;
            }
        }
        i += 1;
    }
    out.push_str(&html[last..]);
    out
}

/// Carattere predefinito della firma, dalla regola `p.MsoNormal` di Word.
fn base_font(source: &str, lower: &str) -> Option<String> {
    let start = lower.find("p.msonormal")?;
    let open = start + lower[start..].find('{')? + 1;
    let close = open + lower[open..].find('}')?;
    let kept: Vec<String> = source[open..close]
        .split(';')
        .filter_map(|d| {
            let (prop, value) = d.split_once(':')?;
            let prop = prop.trim().to_ascii_lowercase();
            let value = value.split_whitespace().collect::<Vec<_>>().join(" ").replace('"', "'");
            matches!(prop.as_str(), "font-family" | "font-size" | "color").then(|| format!("{prop}:{value}"))
        })
        .collect();
    (!kept.is_empty()).then(|| kept.join(";"))
}

/// Outlook salva le firme in windows-1252 (lo dichiara nel `<meta>`), più di
/// rado in UTF-8 o UTF-16.
fn decode_html(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        return String::from_utf16_lossy(&units);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]).to_ascii_lowercase();
    if head.contains("charset=utf-8") || head.contains("charset=\"utf-8\"") {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    bytes.iter().map(|&b| windows_1252(b)).collect()
}

fn windows_1252(b: u8) -> char {
    const HIGH: [u16; 32] = [
        0x20AC, 0x81, 0x201A, 0x192, 0x201E, 0x2026, 0x2020, 0x2021, 0x2C6, 0x2030, 0x160, 0x2039, 0x152, 0x8D, 0x17D,
        0x8F, 0x90, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x2DC, 0x2122, 0x161, 0x203A, 0x153, 0x9D,
        0x17E, 0x178,
    ];
    match b {
        0x80..=0x9F => char::from_u32(HIGH[(b - 0x80) as usize] as u32).unwrap_or('\u{FFFD}'),
        _ => b as char,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("postamon-firme-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// PNG di un pixel.
    const PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0x0D, 0x49, 0x48, 0x44, 0x52, 0, 0, 0, 1, 0, 0, 0, 1,
        8, 6, 0, 0, 0, 0x1F, 0x15, 0xC4, 0x89, 0, 0, 0, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0, 1, 0, 0, 5,
        0, 1, 0x0D, 0x0A, 0x2D, 0xB4, 0, 0, 0, 0, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    #[test]
    fn cleans_what_the_editor_sends() {
        let html = "<script>alert(1)</script><div onclick=\"x()\" class=\"a\" style=\"color:red;mso-bidi-font-family:Calibri\">\
                    <b>Marco</b></div><img src=\"data:image/png;base64,AAAA\" onerror=\"x()\">\
                    <a href=\"data:text/html,ciao\">brutto</a><a href=\"javascript:x()\">peggio</a>\
                    <form><input></form><table><tr><td width=\"80\">logo</td></tr></table>";
        let clean = clean(html);
        assert!(!clean.contains("script") && !clean.contains("onclick") && !clean.contains("onerror"));
        assert!(!clean.contains("class=") && !clean.contains("mso-") && !clean.contains("form"));
        assert!(clean.contains("<div style=\"color:red\"><b>Marco</b></div>"), "{clean}");
        assert!(clean.contains("src=\"data:image/png;base64,AAAA\""));
        assert!(!clean.contains("data:text") && !clean.contains("javascript"));
        assert!(clean.contains("<td width=\"80\">logo</td>"));
        // un'immagine senza sorgente valida non resta come icona rotta
        assert_eq!(super::clean("<b>a</b><img src=x onerror=\"alert(2)\"><img alt=\"vuota\">"), "<b>a</b>");
    }

    #[test]
    fn unwraps_links_rewritten_by_spam_filters() {
        let html = "<a href=\"https://urlsand.esvalabs.com/?u=http%3A%2F%2Fwww.dimensionesalotto.it&amp;e=bd2c&amp;f=y\">sito</a>\
                    <a href=\"https://eur01.safelinks.protection.outlook.com/?url=https%3A%2F%2Fx.it%2Fa&amp;data=1\">x</a>\
                    <a href=\"https://urlsand.esvalabs.com/?u=javascript%3Aalert(1)\">no</a>";
        let clean = clean(html);
        assert!(clean.contains("href=\"http://www.dimensionesalotto.it\""), "{clean}");
        assert!(clean.contains("href=\"https://x.it/a\""), "{clean}");
        assert!(!clean.contains("javascript:alert"), "{clean}");
    }

    #[test]
    fn trims_blank_lines_at_the_edges_only() {
        let html = "<div>&nbsp;</div>\n<div><b>Marco</b></div><div>&nbsp;</div><div>Telefono</div>\
                    <table><tr><td>a</td><td><div>b</div><div><span>&nbsp;</span></div></td></tr></table>\
                    <div><span style=\"font-size:11pt\">&nbsp;</span></div><div><br></div>\n";
        assert_eq!(
            clean(html),
            "<div><b>Marco</b></div><div>&nbsp;</div><div>Telefono</div>\
             <table><tbody><tr><td>a</td><td><div>b</div></td></tr></tbody></table>"
        );
        assert_eq!(clean("<div>&nbsp;</div><div><br></div>"), "");
        // un'immagine da sola non è una riga vuota
        assert_eq!(clean("<div><img src=\"https://x.it/l.png\"></div><div>&nbsp;</div>"), "<div><img src=\"https://x.it/l.png\"></div>");
    }

    #[test]
    fn imports_outlook_signatures_with_images() {
        let dir = temp_dir("outlook");
        let files = dir.join("Marco (m@x.it)_file");
        std::fs::create_dir_all(&files).unwrap();
        std::fs::write(files.join("image001.png"), PNG).unwrap();
        let mut htm = b"<html xmlns:v=\"urn:schemas-microsoft-com:vml\"><head>\
            <meta http-equiv=Content-Type content=\"text/html; charset=windows-1252\">\
            <title>Firma</title><style>p.MsoNormal, li.MsoNormal {margin:0cm;\nfont-size:11.0pt;\
            font-family:\"Calibri\",sans-serif;mso-fareast-font-family:Calibri;}</style>\
            <!--[if gte mso 9]><xml><o:OfficeDocumentSettings>800x600</o:OfficeDocumentSettings></xml><![endif]-->\
            </head><body lang=IT><div class=WordSection1>\
            <p class=MsoNormal><span style='font-size:1.0pt'><!--[if gte vml 1]><v:shape style='width:78pt'>\
            <v:imagedata src=\"Marco%20(m@x.it)_file/image001.png\"/></v:shape><![endif]--><![if !vml]>\
            <img width=105 height=78 src=\"Marco%20(m@x.it)_file/image001.png\" alt=Logo v:shapes=\"Immagine_1\">\
            <![endif]></span><o:p></o:p></p>\
            <p class=MsoNormal style='margin:.05pt'><b>Marco Elefanti</b> \x96 Citt\xE0 \x80<o:p></o:p></p>\
            <p class=MsoNormal><a href=\"https://urlsand.esvalabs.com/?u=https%3A%2F%2Fx.it\">x.it</a></p>\
            <p class=MsoNormal><o:p>&nbsp;</o:p></p><p class=MsoNormal><o:p>&nbsp;</o:p></p>\
            </div></body></html>"
            .to_vec();
        htm.extend_from_slice(b"\r\n");
        std::fs::write(dir.join("Marco (m@x.it).htm"), &htm).unwrap();
        std::fs::write(dir.join("Marco (m@x.it).txt"), b"testo").unwrap();

        let list = outlook_in(&dir);
        assert_eq!(list.len(), 1);
        let html = &list[0].html;
        assert_eq!(list[0].name, "Marco (m@x.it)");
        assert!(html.starts_with("<div style=\"font-size:11.0pt;font-family:'Calibri',sans-serif\">"), "{html}");
        assert!(html.contains("<img width=\"105\" height=\"78\" src=\"data:image/png;base64,"), "{html}");
        assert!(html.contains("<div style=\"margin:.05pt\"><b>Marco Elefanti</b> – Città €</div>"), "{html}");
        assert!(html.contains("href=\"https://x.it\""), "{html}");
        for gone in ["800x600", "Firma", "v:", "o:p", "MsoNormal", "mso-", "<p", "&nbsp;"] {
            assert!(!html.contains(gone), "{gone} in {html}");
        }
        assert_eq!(outlook_email(&list[0].name).as_deref(), Some("m@x.it"));
        assert_eq!(outlook_email("tende marco"), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn relative_images_stay_inside_the_signatures_folder() {
        let dir = temp_dir("fuori");
        let inner = dir.join("firme");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(dir.join("segreto.png"), PNG).unwrap();
        let html = builder(Some(inner)).clean("<img src=\"../segreto.png\"><img src=\"cid:x\">").to_string();
        assert!(!html.contains("data:"), "{html}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn images_become_inline_parts_once() {
        let png = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(PNG));
        let html = format!("<img src=\"{png}\"><b>Marco</b><img src=\"{png}\"><img src=\"https://x.it/a.png\">");
        let (out, images) = for_mail(&html);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].mime, "image/png");
        assert_eq!(images[0].data, PNG);
        assert_eq!(out.matches(&format!("src=\"cid:{}\"", images[0].cid)).count(), 2, "{out}");
        assert!(out.contains("src=\"https://x.it/a.png\"") && !out.contains("data:"));
    }

    #[test]
    fn text_signatures_of_older_versions() {
        let html = from_text("Marco\r\n\r\nDimensione Salotto <www>\nhttps://x.it\n");
        assert_eq!(
            html,
            "<div>Marco</div><div><br></div><div>Dimensione Salotto &lt;www&gt;</div>\
             <div><a href=\"https://x.it\">https://x.it</a></div>"
        );
        assert_eq!(to_text(&html), "Marco\n\nDimensione Salotto <www>\nhttps://x.it");
    }

    #[test]
    fn text_version_of_a_business_card() {
        let html = "<div style=\"font-size:11pt\"><table><tbody><tr><td><div><img src=\"https://x.it/l.png\"></div></td>\
                    <td><table><tbody><tr><td><div><b>Marco</b> Elefanti</div></td></tr>\
                    <tr><td><div>IT&nbsp;Analyst</div></td></tr>\
                    <tr><td><div><a href=\"mailto:m@x.it\">m@x.it</a></div></td></tr>\
                    <tr><td><div><a href=\"https://www.x.it/contatti\">Contatti</a></div></td></tr></tbody></table>\
                    <div><span style=\"display:none\">&nbsp;</span></div>\
                    <table><tbody><tr><td><a href=\"https://fb.com/x\"><img src=\"https://x.it/f.png\" alt=\"Facebook\"></a></td></tr></tbody></table>\
                    </td></tr></tbody></table><div>&nbsp;</div><div>Riservato.</div></div>";
        assert_eq!(to_text(html), "Marco Elefanti\nIT Analyst\nm@x.it\nContatti <https://www.x.it/contatti>\n\nRiservato.");
        assert_eq!(to_text("Marco<div>Tel<br>Fax</div><br><div>x</div>"), "Marco\nTel\nFax\n\nx");
    }

    #[test]
    fn image_files_for_the_editor() {
        let dir = temp_dir("immagini");
        std::fs::write(dir.join("logo.PNG"), PNG).unwrap();
        std::fs::write(dir.join("logo.svg"), "<svg/>").unwrap();
        std::fs::write(dir.join("big.jpg"), vec![0u8; MAX_IMAGE + 1]).unwrap();
        assert!(image(&dir.join("logo.PNG")).unwrap().starts_with("data:image/png;base64,iVBOR"));
        assert!(image(&dir.join("logo.svg")).unwrap_err().to_string().contains("SVG"));
        assert!(image(&dir.join("big.jpg")).unwrap_err().to_string().contains("troppo grande"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn saves_and_reloads_the_list() {
        let dir = temp_dir("elenco");
        let path = dir.join("signatures.json");
        let (sigs, fresh) = Signatures::load(path.clone()).unwrap();
        assert!(fresh);
        let a = sigs.save(None, " Marco ", "<b onclick=\"x\">Marco</b>").unwrap();
        assert_eq!((a.name.as_str(), a.html.as_str()), ("Marco", "<b>Marco</b>"));
        assert!(sigs.save(None, "  ", "x").is_err());
        assert!(sigs.save(Some("nessuna"), "X", "x").is_err());
        sigs.save(Some(&a.id), "Marco 2", "ciao").unwrap();
        let added = sigs.add(vec![("Outlook".into(), "<i>o</i>".into())]).unwrap();
        let (again, fresh) = Signatures::load(path).unwrap();
        assert!(!fresh);
        assert_eq!(again.all().len(), 2);
        assert_eq!(again.get(&a.id).unwrap().name, "Marco 2");
        again.remove(&added[0].id).unwrap();
        assert!(again.get(&added[0].id).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}

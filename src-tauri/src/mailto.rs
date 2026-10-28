//! Postamon come programma per i link email (`mailto:`).
//!
//! - A ogni avvio della versione installata Postamon si registra, per
//!   l'utente, fra i programmi che aprono i link email: così Windows lo
//!   propone quando se ne clicca uno e lo elenca in Impostazioni > App
//!   predefinite. Diventare il predefinito lo decide l'utente: Windows non
//!   lascia che un programma lo imposti da solo.
//! - Il link arriva come argomento (`postamon.exe "mailto:..."`). Al primo
//!   avvio resta in attesa finché l'interfaccia lo chiede; se Postamon è già
//!   aperto glielo passa l'istanza unica, con l'evento `mailto`.
//! - Dal link si prendono destinatari, oggetto e testo; gli allegati
//!   (`attach=`, che qualche programma accetta) no: un link su una pagina web
//!   potrebbe far partire un file del PC.
//! - La disinstallazione toglie la registrazione (`windows/hooks.nsh`).

use serde::Serialize;

/// Nuovo messaggio chiesto da un link email.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mailto {
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub body: String,
}

/// `mailto:a@x.it,b@y.it?cc=c@z.it&subject=Ciao&body=Riga%201%0D%0ARiga%202`
pub fn parse(link: &str) -> Option<Mailto> {
    let link = link.trim();
    let rest = link.get(..7).filter(|p| p.eq_ignore_ascii_case("mailto:")).map(|_| &link[7..])?;
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (mut to, mut cc, mut bcc, mut body) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut subject = String::new();
    add_addresses(&mut to, &decode(path));
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = decode(value);
        match decode(key).to_ascii_lowercase().as_str() {
            "to" => add_addresses(&mut to, &value),
            "cc" => add_addresses(&mut cc, &value),
            "bcc" => add_addresses(&mut bcc, &value),
            "subject" => subject = value.replace(['\r', '\n'], " ").trim().to_string(),
            "body" => body.push(value.replace("\r\n", "\n").replace('\r', "\n")),
            _ => {}
        }
    }
    Some(Mailto { to: to.join(", "), cc: cc.join(", "), bcc: bcc.join(", "), subject, body: body.join("\n") })
}

/// Il primo argomento che è un link email.
pub fn from_args<I: IntoIterator<Item = S>, S: AsRef<str>>(args: I) -> Option<Mailto> {
    args.into_iter().find_map(|a| parse(a.as_ref()))
}

fn add_addresses(list: &mut Vec<String>, value: &str) {
    list.extend(value.split(',').map(str::trim).filter(|a| !a.is_empty()).map(str::to_string));
}

/// Decodifica `%XX` (UTF-8). Il `+` resta un `+`: nei link email non è uno spazio.
fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%'
            && let (Some(h), Some(l)) = (bytes.get(i + 1).and_then(|b| hex(*b)), bytes.get(i + 2).and_then(|b| hex(*b)))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Pagina di Impostazioni con i link e i file che Postamon sa aprire.
pub fn settings_url(name: &str) -> String {
    format!("ms-settings:defaultapps?registeredAppUser={name}")
}

/// Registra Postamon (`name`, di solito "Postamon") per l'utente come
/// programma per i link email. Non fa niente se è già registrato da questo
/// eseguibile.
#[cfg(windows)]
#[cfg_attr(debug_assertions, allow(dead_code))]
pub fn register(name: &str, description: &str) -> anyhow::Result<()> {
    use windows_registry::CURRENT_USER;

    let exe = std::env::current_exe()?.to_string_lossy().into_owned();
    let prog_id = format!("{name}.mailto");
    let class = format!(r"Software\Classes\{prog_id}");
    let capabilities = format!(r"Software\{name}\Capabilities");
    let command = format!("\"{exe}\" \"%1\"");
    let icon = format!("{exe},0");

    let registered = CURRENT_USER.open(format!(r"{class}\shell\open\command")).and_then(|k| k.get_string(""));
    let listed = CURRENT_USER.open(r"Software\RegisteredApplications").and_then(|k| k.get_string(name));
    if registered.is_ok_and(|c| c == command) && listed.is_ok_and(|c| c == capabilities) {
        return Ok(());
    }

    let key = CURRENT_USER.create(&class)?;
    key.set_string("", format!("Link email ({name})"))?;
    key.set_string("FriendlyTypeName", format!("Link email ({name})"))?;
    key.set_string("URL Protocol", "")?;
    CURRENT_USER.create(format!(r"{class}\DefaultIcon"))?.set_string("", &icon)?;
    CURRENT_USER.create(format!(r"{class}\shell\open\command"))?.set_string("", &command)?;
    let app = CURRENT_USER.create(format!(r"{class}\Application"))?;
    app.set_string("ApplicationName", name)?;
    app.set_string("ApplicationIcon", &icon)?;

    let caps = CURRENT_USER.create(&capabilities)?;
    caps.set_string("ApplicationName", name)?;
    caps.set_string("ApplicationDescription", description)?;
    caps.set_string("ApplicationIcon", &icon)?;
    CURRENT_USER.create(format!(r"{capabilities}\URLAssociations"))?.set_string("mailto", &prog_id)?;
    CURRENT_USER.create(r"Software\RegisteredApplications")?.set_string(name, &capabilities)?;

    // Avvisa Esplora risorse che i programmi associati sono cambiati.
    unsafe {
        use windows_sys::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
        SHChangeNotify(SHCNE_ASSOCCHANGED as _, SHCNF_IDLIST, std::ptr::null(), std::ptr::null());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_recipients_subject_and_body() {
        let m = parse("mailto:Anna%40x.it, luca@y.it?cc=info@ds.it&Subject=Preventivo%20divano&body=Buongiorno%2C%0D%0Ala%20misura%20%C3%A8%20240+cm")
            .unwrap();
        assert_eq!(m.to, "Anna@x.it, luca@y.it");
        assert_eq!(m.cc, "info@ds.it");
        assert_eq!(m.subject, "Preventivo divano");
        assert_eq!(m.body, "Buongiorno,\nla misura è 240+cm");
    }

    #[test]
    fn accepts_empty_and_uppercase_links() {
        assert_eq!(parse("mailto:"), Some(Mailto::default()));
        assert_eq!(parse("MAILTO:marco@ds.it").unwrap().to, "marco@ds.it");
        assert_eq!(parse("mailto:?to=a@x.it&to=b@x.it&bcc=c@x.it").unwrap().to, "a@x.it, b@x.it");
    }

    #[test]
    fn ignores_attachments_and_other_links() {
        let m = parse(r"mailto:a@x.it?attach=C:\Users\marco\segreto.txt&subject=Ciao").unwrap();
        assert_eq!(m, Mailto { to: "a@x.it".into(), subject: "Ciao".into(), ..Default::default() });
        assert_eq!(parse("https://ds.it"), None);
        assert_eq!(parse("mail"), None);
        assert_eq!(parse("mailto:%E0%A4"), Some(Mailto { to: "\u{fffd}".into(), ..Default::default() }));
    }

    #[test]
    fn finds_the_link_among_the_arguments() {
        let args = ["C:\\Postamon\\postamon.exe", "mailto:a@x.it?subject=Ordine%2012"];
        assert_eq!(from_args(args).unwrap().subject, "Ordine 12");
        assert_eq!(from_args(["postamon.exe"]), None);
    }
}

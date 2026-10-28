//! Prova completa contro un server di posta di test (GreenMail in Docker):
//! invio SMTP, sincronizzazione IMAP, apertura, flag, copia in "Posta
//! inviata", cestino. Senza la variabile POSTAMON_E2E viene saltata.
//!
//! ```text
//! docker run -d --name postamon-greenmail -p 3993:3993 -p 3465:3465 \
//!   -e GREENMAIL_OPTS="-Dgreenmail.setup.test.all -Dgreenmail.hostname=0.0.0.0 \
//!   -Dgreenmail.users=marco:segreta@localhost -Dgreenmail.users.login=email" greenmail/standalone
//! POSTAMON_E2E=1 cargo test e2e -- --nocapture
//! ```

use crate::accounts::Account;
use crate::store::Store;
use crate::{imap, render, smtp};

const PASSWORD: &str = "segreta";

fn account() -> Account {
    Account {
        id: "marco@localhost".into(),
        email: "marco@localhost".into(),
        name: "Marco".into(),
        imap_host: "127.0.0.1".into(),
        imap_port: 3993,
        smtp_host: "127.0.0.1".into(),
        smtp_port: 3465,
        signature_new: None,
        signature_reply: None,
        signature: String::new(),
        accept_invalid_certs: true,
    }
}

#[tokio::test]
async fn send_sync_open_and_move() {
    if std::env::var_os("POSTAMON_E2E").is_none() {
        eprintln!("POSTAMON_E2E non impostata: prova saltata");
        return;
    }
    let acc = account();
    let subject = format!("Prova e2e {}", std::process::id());

    let message = smtp::build(
        &acc,
        smtp::Draft {
            to: "Marco <marco@localhost>",
            cc: "",
            bcc: "",
            subject: &subject,
            body: "Ciao, il listino è qui: https://dimensionesalotto.it/listino",
            quote: "",
            signature: None,
            files: vec![smtp::OutgoingFile {
                name: "nota.txt".into(),
                mime: "text/plain".into(),
                data: "contenuto con àèì".as_bytes().to_vec(),
            }],
            in_reply_to: None,
            references: None,
        },
    )
    .unwrap();
    let raw_sent = smtp::send(&acc, PASSWORD, &message).await.unwrap();

    let store = Store::in_memory().unwrap();
    let mut c = imap::connect(&acc, PASSWORD).await.unwrap();
    for name in ["Sent", "Trash"] {
        let _ = c.session.create(name).await;
    }
    let folders = imap::list_folders(&mut c).await.unwrap();
    store.replace_folders(&acc.id, &folders).unwrap();
    assert_eq!(store.folder_with_role(&acc.id, "sent").unwrap().as_deref(), Some("Sent"));
    assert_eq!(store.folder_with_role(&acc.id, "trash").unwrap().as_deref(), Some("Trash"));

    imap::sync_folder(&mut c, &store, &acc.id, "INBOX").await.unwrap();
    let list = store.list(&acc.id, "INBOX", 500, 0).unwrap();
    let m = list.iter().find(|m| m.subject == subject).expect("messaggio arrivato").clone();
    assert_eq!(m.from_name, "Marco");
    assert!(m.has_attachments);
    assert!(!m.seen);

    let raw = imap::fetch_body(&mut c, "INBOX", m.uid).await.unwrap();
    let r = render::render(&raw, false).unwrap();
    assert!(r.html.contains("il listino è qui"));
    assert!(r.html.contains("href=\"https://dimensionesalotto.it/listino\""));
    assert_eq!(r.attachments.len(), 1);
    let (name, _, data) = render::attachment_data(&raw, r.attachments[0].index).unwrap();
    assert_eq!(name, "nota.txt");
    assert_eq!(String::from_utf8(data).unwrap(), "contenuto con àèì");

    // Letto sul server → letto anche in archivio alla sincronizzazione successiva.
    imap::set_flag(&mut c, "INBOX", m.uid, "\\Seen", true).await.unwrap();
    imap::sync_folder(&mut c, &store, &acc.id, "INBOX").await.unwrap();
    assert!(store.summary(&acc.id, "INBOX", m.uid).unwrap().unwrap().seen);

    // Copia in Posta inviata.
    imap::append(&mut c, "Sent", &raw_sent, "(\\Seen)").await.unwrap();
    imap::sync_folder(&mut c, &store, &acc.id, "Sent").await.unwrap();
    assert!(store.list(&acc.id, "Sent", 500, 0).unwrap().iter().any(|s| s.subject == subject));

    // Nel cestino: sparisce dalla posta in arrivo, compare nel cestino.
    imap::delete(&mut c, "INBOX", m.uid, Some("Trash")).await.unwrap();
    imap::sync_folder(&mut c, &store, &acc.id, "INBOX").await.unwrap();
    assert!(store.summary(&acc.id, "INBOX", m.uid).unwrap().is_none());
    imap::sync_folder(&mut c, &store, &acc.id, "Trash").await.unwrap();
    assert!(store.list(&acc.id, "Trash", 500, 0).unwrap().iter().any(|s| s.subject == subject));

    let _ = c.session.logout().await;
}

/// Prepara una cartella dati di prova con la casella del server di test e
/// qualche messaggio realistico, per guardare l'interfaccia:
/// POSTAMON_SEED=<cartella> cargo test seed_demo -- --nocapture
#[tokio::test]
async fn seed_demo() {
    let Some(dir) = std::env::var_os("POSTAMON_SEED") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let acc = account();
    std::fs::write(dir.join("accounts.json"), serde_json::to_vec_pretty(&vec![acc.clone()]).unwrap()).unwrap();
    crate::accounts::set_password(&acc.email, PASSWORD).unwrap();

    let mut c = imap::connect(&acc, PASSWORD).await.unwrap();
    for name in ["Sent", "Trash", "Drafts", "Clienti"] {
        let _ = c.session.create(name).await;
    }
    let messages = [
        (
            "Anna Ferrari <anna.ferrari@arredamentiferrari.it>",
            "Preventivo divano Ergoluna 3 posti",
            "Mon, 19 Oct 2026 16:05:00 +0200",
            "text/plain; charset=utf-8",
            "Buongiorno Marco,\r\n\r\nle mando in allegato il preventivo aggiornato per il divano Ergoluna.\r\nLa consegna è prevista entro fine novembre.\r\n\r\nMi faccia sapere, grazie.\r\nAnna Ferrari\r\nhttps://arredamentiferrari.it",
            true,
        ),
        (
            "Luca Bianchi <luca.bianchi@gmail.com>",
            "Conferma appuntamento sabato",
            "Tue, 20 Oct 2026 09:12:00 +0200",
            "text/plain; charset=utf-8",
            "Ciao, confermo sabato alle 10:30 nel negozio di Villa Cortese.\r\nPorto le misure del soggiorno.\r\n\r\nLuca",
            false,
        ),
        (
            "Offerte Tessuti Brianza <news@tessutibrianza.it>",
            "Nuova collezione autunno: velluti e bouclé",
            "Wed, 21 Oct 2026 07:30:00 +0200",
            "text/html; charset=utf-8",
            "<html><head><style>.hero{background:#2f4858;color:#fff;padding:28px;font-family:Georgia,serif}.cta{display:inline-block;background:#c8553d;color:#fff;padding:10px 18px;border-radius:4px;text-decoration:none}</style></head><body><table width=\"600\" align=\"center\" cellpadding=\"0\" cellspacing=\"0\"><tr><td class=\"hero\"><h1 style=\"margin:0\">Collezione Autunno 2026</h1><p>Velluti, bouclé e lini lavati per i vostri divani.</p></td></tr><tr><td style=\"padding:20px\"><img src=\"https://cdn.tessutibrianza.it/img/autunno.jpg\" width=\"560\" alt=\"Campionario\"><p>Il campionario completo è disponibile su richiesta.</p><p><a class=\"cta\" href=\"https://tessutibrianza.it/autunno\">Scopri la collezione</a></p><img src=\"https://track.tessutibrianza.it/open.gif?id=123\" width=\"1\" height=\"1\"></td></tr></table><script>document.write('x')</script></body></html>",
            false,
        ),
        (
            "Giulia Romano <info@dimensionesalotto.it>",
            "Turni di novembre",
            "Wed, 21 Oct 2026 18:40:00 +0200",
            "text/plain; charset=utf-8",
            "Ciao a tutti,\r\nin allegato trovate i turni di novembre per Arconate e Villa Cortese.\r\n\r\nGiulia",
            true,
        ),
        (
            "Mobilificio Rossi <ordini@mobilificiorossi.it>",
            "Ordine 2026/1187 spedito",
            "Thu, 22 Oct 2026 08:15:00 +0200",
            "text/html; charset=utf-8",
            "<p>Gentile cliente,</p><p>l'ordine <b>2026/1187</b> è stato affidato al corriere.</p><table border=\"1\" cellpadding=\"6\" style=\"border-collapse:collapse\"><tr><th>Articolo</th><th>Q.tà</th></tr><tr><td>Poltrona relax Zeta</td><td>2</td></tr><tr><td>Pouf contenitore</td><td>1</td></tr></table><p>Traccia la spedizione: <a href=\"https://tracking.example.it/2026-1187\">clicca qui</a></p>",
            false,
        ),
    ];
    for (from, subject, date, ctype, body, with_pdf) in messages {
        let encoded_subject = format!("=?utf-8?B?{}?=", base64::Engine::encode(&base64::engine::general_purpose::STANDARD, subject));
        let mut raw = format!(
            "From: {from}\r\nTo: Marco <marco@localhost>\r\nSubject: {encoded_subject}\r\nDate: {date}\r\n\
             Message-ID: <{}@demo>\r\nMIME-Version: 1.0\r\n",
            subject.len() * 7 + body.len()
        );
        if with_pdf {
            let pdf = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, sample_pdf());
            raw.push_str(&format!(
                "Content-Type: multipart/mixed; boundary=\"zz\"\r\n\r\n--zz\r\nContent-Type: {ctype}\r\nContent-Transfer-Encoding: 8bit\r\n\r\n{body}\r\n\
                 --zz\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=\"{}.pdf\"\r\n\
                 Content-Transfer-Encoding: base64\r\n\r\n{pdf}\r\n--zz--\r\n",
                if subject.starts_with("Turni") { "Turni novembre 2026" } else { "Preventivo Ergoluna" }
            ));
        } else {
            raw.push_str(&format!("Content-Type: {ctype}\r\nContent-Transfer-Encoding: 8bit\r\n\r\n{body}\r\n"));
        }
        imap::append(&mut c, "INBOX", raw.as_bytes(), "()").await.unwrap();
    }
    let _ = c.session.logout().await;
    eprintln!("dati di prova pronti in {}", dir.display());
}

/// PDF minimo di una pagina con un titolo, per provare le anteprime.
fn sample_pdf() -> Vec<u8> {
    let stream = "BT /F1 30 Tf 72 740 Td (Preventivo Ergoluna 3 posti) Tj ET\nBT /F1 14 Tf 72 700 Td (Divano in velluto, consegna entro fine novembre.) Tj ET";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_string(),
        format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    let mut pdf = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.push_str(&format!("{} 0 obj\n{body}\nendobj\n", i + 1));
    }
    let xref = pdf.len();
    pdf.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1));
    for o in offsets {
        pdf.push_str(&format!("{o:010} 00000 n \n"));
    }
    pdf.push_str(&format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1));
    pdf.into_bytes()
}

/// Server delle anteprime con PdfCraft e PhotoCraft web e due file di prova,
/// acceso per POSTAMON_CRAFT_SERVE secondi, per provarli in un browser.
/// Gli indirizzi finiscono in `<POSTAMON_CRAFT_ROOT>/urls.json`.
#[tokio::test]
async fn craft_serve() {
    let Some(secs) = std::env::var("POSTAMON_CRAFT_SERVE").ok().and_then(|s| s.parse::<u64>().ok()) else {
        return;
    };
    let root = std::path::PathBuf::from(std::env::var_os("POSTAMON_CRAFT_ROOT").expect("POSTAMON_CRAFT_ROOT"));
    let craft = crate::craft::Craft::start(root.clone()).unwrap();
    let pdf = crate::craft::viewer_for("x.pdf").unwrap();
    let photo = crate::craft::viewer_for("x.jpg").unwrap();
    for app in [pdf, photo] {
        let started = std::time::Instant::now();
        craft.install_web(app, |_, _| {}).await.unwrap();
        eprintln!("{} pronto in {:?}", app.name, started.elapsed());
    }
    let (pdf_url, _) = craft.open(pdf, "Preventivo Ergoluna.pdf", "application/pdf", sample_pdf());
    let jpg = std::fs::read(r"C:\Windows\Web\Wallpaper\Windows\img0.jpg").unwrap();
    let (photo_url, _) = craft.open(photo, "salotto.jpg", "image/jpeg", jpg);
    let urls = serde_json::json!({ "pdf": pdf_url, "photo": photo_url });
    std::fs::write(root.join("urls.json"), urls.to_string()).unwrap();
    eprintln!("indirizzi: {urls}");
    tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
}

/// Cartella dati di prova senza server di posta: una casella che non si
/// collega e due messaggi già in archivio, uno con un PDF e uno con una foto,
/// per provare le anteprime ArtCraft dentro l'app:
/// POSTAMON_SEED_OFFLINE=<cartella> cargo test seed_offline
#[test]
fn seed_offline() {
    let Some(dir) = std::env::var_os("POSTAMON_SEED_OFFLINE") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut acc = account();
    acc.imap_port = 1;
    acc.smtp_port = 1;
    // Seconda casella, per vedere gli spazi di lavoro.
    let shop = Account {
        id: "info@localhost".into(),
        email: "info@localhost".into(),
        name: "Info - Negozio".into(),
        ..acc.clone()
    };
    std::fs::write(dir.join("accounts.json"), serde_json::to_vec_pretty(&vec![acc.clone(), shop.clone()]).unwrap())
        .unwrap();
    let store = Store::open(&dir.join("posta.sqlite")).unwrap();
    for a in [&acc, &shop] {
        store
            .replace_folders(
                &a.id,
                &[
                    crate::store::FolderInfo {
                        name: "INBOX".into(),
                        display: "Posta in arrivo".into(),
                        role: Some("inbox"),
                        depth: 0,
                        selectable: true,
                        position: 0,
                    },
                    crate::store::FolderInfo {
                        name: "Sent".into(),
                        display: "Posta inviata".into(),
                        role: Some("sent"),
                        depth: 0,
                        selectable: true,
                        position: 2,
                    },
                ],
            )
            .unwrap();
    }
    let b64 = |data: &[u8]| base64::Engine::encode(&base64::engine::general_purpose::STANDARD, data);
    let mail = |from: &str, subject: &str, date: &str, body: &str, file: &str, mime: &str, data: &[u8]| {
        format!(
            "From: {from}\r\nTo: Marco <marco@localhost>\r\nSubject: {subject}\r\nDate: {date}\r\nMIME-Version: 1.0\r\n\
             Content-Type: multipart/mixed; boundary=\"zz\"\r\n\r\n--zz\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body}\r\n\
             --zz\r\nContent-Type: {mime}\r\nContent-Disposition: attachment; filename=\"{file}\"\r\n\
             Content-Transfer-Encoding: base64\r\n\r\n{}\r\n--zz--\r\n",
            b64(data)
        )
        .into_bytes()
    };
    let photo = std::fs::read(r"C:\Windows\Web\Wallpaper\Windows\img0.jpg").unwrap();
    let messages = [
        mail(
            "Anna Ferrari <anna.ferrari@arredamentiferrari.it>",
            "Preventivo divano Ergoluna",
            "Thu, 22 Oct 2026 09:05:00 +0200",
            "Buongiorno Marco, in allegato il preventivo aggiornato.",
            "Preventivo Ergoluna.pdf",
            "application/pdf",
            &sample_pdf(),
        ),
        mail(
            "Luca Bianchi <luca.bianchi@gmail.com>",
            "Foto del soggiorno",
            "Thu, 22 Oct 2026 10:40:00 +0200",
            "Ciao, ecco la foto del soggiorno per il divano.",
            "soggiorno.jpg",
            "image/jpeg",
            &photo,
        ),
    ];
    let shop_mail = mail(
        "Giulia Verdi <giulia.verdi@gmail.com>",
        "Richiesta preventivo divano angolare",
        "Thu, 22 Oct 2026 11:20:00 +0200",
        "Buongiorno, vorrei un preventivo per il divano angolare visto in negozio.",
        "misure soggiorno.txt",
        "text/plain",
        "Parete lunga 3,20 m\nParete corta 2,10 m".as_bytes(),
    );
    let all = messages.iter().map(|m| (&acc, m)).chain([(&shop, &shop_mail)]);
    let mut uids: std::collections::HashMap<String, u32> = Default::default();
    for (a, raw) in all {
        let uid = uids.entry(a.id.clone()).and_modify(|u| *u += 1).or_insert(1);
        let mut row = render::header_row(raw);
        row.uid = *uid;
        row.size = raw.len() as u32;
        store.insert_headers(&a.id, "INBOX", &[row]).unwrap();
        let (preview, has_attachments) = render::digest(raw);
        store.put_body(&a.id, "INBOX", *uid, raw, &preview, has_attachments).unwrap();
    }
    // Contatori come li darebbe il server.
    store.set_counts(&acc.id, "INBOX", 2, 2).unwrap();
    store.set_counts(&shop.id, "INBOX", 1, 1).unwrap();
    // Rubrica: Anna è già un contatto (segno nella lista), Luca no (icona per aggiungerlo).
    // Arredamenti Ferrari ha due domini e Sara le scrive da Gmail; Marta da Gmail per Tappezzeria Ronchi.
    let contacts = [
        ("anna.ferrari@arredamentiferrari.it", "Anna Ferrari", "Arredamenti Ferrari", "0331 555 214"),
        ("paolo.ferrari@arredamentiferrari.it", "Paolo Ferrari", "Arredamenti Ferrari", ""),
        ("giorgio@ferrari-arredi.com", "Giorgio Ferrari", "Arredamenti Ferrari", ""),
        ("sara.galli@gmail.com", "Sara Galli", "Arredamenti Ferrari", "339 555 7710"),
        ("marta.villa@gmail.com", "Marta Villa", "Tappezzeria Ronchi", ""),
        ("ordini@tappezzeriaronchi.it", "Ufficio ordini", "Tappezzeria Ronchi", ""),
        ("elena.rizzo@libero.it", "Elena Rizzo", "", ""),
    ];
    for (address, name, company, phone) in contacts {
        let input = crate::store::ContactInput {
            address: address.into(),
            name: name.into(),
            company: company.into(),
            phone: phone.into(),
            ..Default::default()
        };
        store.save_contact(&acc.id, &input).unwrap();
    }
    let ferrari = crate::store::CompanyInput {
        previous_name: "Arredamenti Ferrari".into(),
        name: "Arredamenti Ferrari".into(),
        vat: "IT01234567890".into(),
        phone: "0331 555 200".into(),
        address: "Via dei Mobili 12\n20020 Arconate (MI)".into(),
        website: "https://www.arredamentiferrari.it".into(),
        ..Default::default()
    };
    store.save_company(&acc.id, &ferrari).unwrap();
    eprintln!("archivio di prova pronto in {}", dir.display());
}

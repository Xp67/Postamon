//! Pagina di download di Postamon: `/scarica` (e `/`, che ci porta).
//!
//! Chi deve installare Postamon non deve sapere il nome del file: la pagina
//! legge `aggiornamenti/latest.json`, lo stesso dei Postamon che si aggiornano
//! da soli, e mostra l'ultima versione per Windows e per Android, con un QR
//! per scaricare l'APK dal telefono. `/scarica/windows` e `/scarica/android`
//! portano sempre al file dell'ultima versione: sono gli indirizzi da mettere
//! nei collegamenti. Niente accesso richiesto, come per gli aggiornamenti.

use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::get;
use serde_json::Value;

use crate::api::Shared;

const ICON: &[u8] = include_bytes!("../../src-tauri/icons/128x128.png");

pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/", get(|| async { Redirect::to("/scarica") }))
        .route("/scarica", get(page))
        .route("/scarica/icona.png", get(icon))
        .route("/scarica/{platform}", get(latest_file))
}

/// Un pacchetto pubblicato per una piattaforma.
#[derive(Debug, PartialEq)]
struct Package {
    version: String,
    date: String,
    notes: String,
    file: String,
}

/// Il pacchetto di `platform` (`windows` o `android`) in `latest.json`.
fn package(manifest: &Value, platform: &str) -> Option<Package> {
    let key = match platform {
        "windows" => "windows-x86_64",
        "android" => "android-aarch64",
        _ => return None,
    };
    let file = manifest["platforms"][key]["url"].as_str()?;
    // Solo il nome del file, come lo scrive pubblica.ps1 (niente cartelle).
    if file.is_empty() || file.starts_with('.') || !file.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)) {
        return None;
    }
    // "2026-10-22T15:45:06Z" → "22/10/2026"
    let date = manifest["pub_date"].as_str().unwrap_or("");
    let date = match (date.get(0..4), date.get(5..7), date.get(8..10)) {
        (Some(y), Some(m), Some(d)) => format!("{d}/{m}/{y}"),
        _ => String::new(),
    };
    Some(Package {
        version: manifest["version"].as_str().unwrap_or("").to_string(),
        date,
        notes: manifest["notes"].as_str().unwrap_or("").to_string(),
        file: file.to_string(),
    })
}

async fn manifest(state: &Shared) -> Value {
    let path = state.cfg.data_dir.join("aggiornamenti").join("latest.json");
    match tokio::fs::read(path).await {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        Err(_) => Value::Null,
    }
}

async fn latest_file(State(state): State<Shared>, Path(platform): Path<String>) -> Response {
    match package(&manifest(&state).await, &platform) {
        Some(p) => Redirect::to(&format!("/api/aggiornamenti/{}", p.file)).into_response(),
        None => (StatusCode::NOT_FOUND, Html(missing_page())).into_response(),
    }
}

async fn icon() -> Response {
    ([(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "max-age=86400")], ICON).into_response()
}

async fn page(State(state): State<Shared>, headers: HeaderMap) -> Html<String> {
    let manifest = manifest(&state).await;
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
    let scheme = if state.cfg.tls { "https" } else { "http" };
    Html(render(&manifest, &format!("{scheme}://{host}")))
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// QR in SVG dell'indirizzo, da mettere direttamente nella pagina.
fn qr_svg(url: &str) -> Option<String> {
    let svg = qrcode::QrCode::new(url.as_bytes())
        .ok()?
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(176, 176)
        .dark_color(qrcode::render::svg::Color("#14222c"))
        .light_color(qrcode::render::svg::Color("#ffffff"))
        .build();
    // Il renderer mette davanti la dichiarazione XML, che dentro l'HTML non va.
    svg.find("<svg").map(|start| svg[start..].to_string())
}

const DOWNLOAD_ICON: &str = r#"<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3v12m0 0-5-5m5 5 5-5M5 21h14"/></svg>"#;

fn help(text: &str) -> String {
    format!(
        r#"<span class="help"><button type="button" aria-label="Come si installa">?</button><span class="tip" role="tooltip">{text}</span></span>"#
    )
}

fn card_windows(p: Option<&Package>) -> String {
    let Some(p) = p else {
        return r#"<section class="card"><h2>Windows</h2><p class="none">Non ancora pubblicato.</p></section>"#.into();
    };
    let tip = help(
        "Il certificato del server è dell'azienda: se il browser avvisa che la connessione non è privata, \
         <b>Avanzate</b> → <b>Continua</b>. Se Windows mostra «PC protetto»: <b>Ulteriori informazioni</b> → \
         <b>Esegui comunque</b>. Una volta installato, Postamon si aggiorna da solo.",
    );
    format!(
        r#"<section class="card"><h2>Windows {tip}</h2>
<p class="version">Versione {version}{date}</p>{notes}
<a class="button" href="/scarica/windows">{DOWNLOAD_ICON}<span>Scarica per Windows</span></a></section>"#,
        version = escape(&p.version),
        date = if p.date.is_empty() { String::new() } else { format!(" · {}", p.date) },
        notes = if p.notes.is_empty() { String::new() } else { format!(r#"<p class="notes">{}</p>"#, escape(&p.notes)) },
    )
}

fn card_android(p: Option<&Package>, base: &str) -> String {
    let Some(p) = p else {
        return r#"<section class="card"><h2>Android</h2><p class="none">In arrivo.</p></section>"#.into();
    };
    let tip = help(
        "Il telefono deve essere nella rete del negozio (Wi-Fi) o in VPN. Se il browser avvisa del certificato: \
         <b>Avanzate</b> → <b>Continua</b>. Alla prima installazione Android chiede di permettere le app da questa \
         fonte: va concesso una volta. Poi Postamon si aggiorna da solo.",
    );
    let qr = qr_svg(&format!("{base}/scarica/android"))
        .map(|svg| format!(r#"<figure class="qr">{svg}<figcaption>Inquadra con il telefono</figcaption></figure>"#))
        .unwrap_or_default();
    format!(
        r#"<section class="card"><h2>Android {tip}</h2>
<p class="version">Versione {version}{date}</p>{notes}
<a class="button" href="/scarica/android">{DOWNLOAD_ICON}<span>Scarica per Android</span></a>{qr}</section>"#,
        version = escape(&p.version),
        date = if p.date.is_empty() { String::new() } else { format!(" · {}", p.date) },
        notes = if p.notes.is_empty() { String::new() } else { format!(r#"<p class="notes">{}</p>"#, escape(&p.notes)) },
    )
}

fn render(manifest: &Value, base: &str) -> String {
    let windows = package(manifest, "windows");
    let android = package(manifest, "android");
    format!(
        r#"<!doctype html>
<html lang="it">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Scarica Postamon</title>
<link rel="icon" href="/scarica/icona.png">
<style>{STYLE}</style>
</head>
<body>
<main>
<header><img src="/scarica/icona.png" alt="" width="56" height="56"><div><h1>Postamon</h1><p>Posta e chat interna</p></div></header>
<div class="cards">{windows}{android}</div>
</main>
</body>
</html>"#,
        windows = card_windows(windows.as_ref()),
        android = card_android(android.as_ref(), base),
    )
}

fn missing_page() -> String {
    format!(
        r#"<!doctype html><html lang="it"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Scarica Postamon</title><style>{STYLE}</style></head>
<body><main><section class="card"><h2>Non ancora pubblicato</h2><p class="none">Questa versione di Postamon non è ancora disponibile.</p><a class="button" href="/scarica"><span>Torna alla pagina di download</span></a></section></main></body></html>"#
    )
}

const STYLE: &str = r#"
:root { --paper: #e6edf2; --sheet: #ffffff; --ink: #14222c; --muted: #5f6b76; --line: #d5dee5; --accent: #0f6f8f; --accent-ink: #ffffff; }
@media (prefers-color-scheme: dark) {
  :root { --paper: #0f171d; --sheet: #17232b; --ink: #e4ecf1; --muted: #9aa8b3; --line: #2a3a45; --accent: #3aa0c2; --accent-ink: #0b141a; }
}
* { box-sizing: border-box; }
body { margin: 0; background: var(--paper); color: var(--ink); font: 15px/1.5 "Segoe UI", system-ui, sans-serif; }
main { max-width: 760px; margin: 0 auto; padding: 40px 16px; }
header { display: flex; align-items: center; gap: 16px; margin-bottom: 28px; }
header h1 { margin: 0; font-size: 28px; }
header p { margin: 0; color: var(--muted); }
.cards { display: grid; grid-template-columns: repeat(auto-fit, minmax(280px, 1fr)); gap: 16px; align-items: start; }
.card { background: var(--sheet); border: 1px solid var(--line); border-radius: 12px; padding: 22px; display: flex; flex-direction: column; gap: 10px; }
.card h2 { margin: 0; font-size: 19px; display: flex; align-items: center; gap: 8px; }
.version { margin: 0; color: var(--muted); }
.notes { margin: 0; }
.none { margin: 0; color: var(--muted); }
.button { margin-top: 6px; display: inline-flex; align-items: center; justify-content: center; gap: 8px; padding: 11px 16px; border-radius: 8px; background: var(--accent); color: var(--accent-ink); font-weight: 600; text-decoration: none; }
.button:hover { filter: brightness(1.08); }
.button svg { width: 18px; height: 18px; fill: none; stroke: currentColor; stroke-width: 2.2; stroke-linecap: round; stroke-linejoin: round; }
.qr { margin: 8px 0 0; display: flex; flex-direction: column; align-items: center; gap: 6px; color: var(--muted); font-size: 13px; }
.qr svg { width: 176px; height: 176px; border-radius: 8px; border: 1px solid var(--line); }
.help { position: relative; display: inline-flex; }
.help button { width: 20px; height: 20px; border-radius: 50%; border: 1px solid var(--line); background: transparent; color: var(--muted); font: 600 12px/1 "Segoe UI", system-ui, sans-serif; cursor: help; padding: 0; }
.help .tip { position: absolute; left: 0; top: 26px; z-index: 2; width: min(300px, 80vw); padding: 10px 12px; border-radius: 8px; background: var(--ink); color: var(--paper); font-size: 13px; font-weight: 400; line-height: 1.45; opacity: 0; visibility: hidden; transition: opacity .15s, visibility 0s .15s; }
.help:hover .tip { opacity: 1; visibility: visible; transition: opacity .15s 1.5s, visibility 0s 1.5s; }
.help button:focus + .tip { opacity: 1; visibility: visible; transition: opacity .15s; }
@media (max-width: 480px) { main { padding: 24px 16px; } header h1 { font-size: 24px; } }
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn manifest() -> Value {
        json!({
            "version": "0.2.0",
            "notes": "Rubrica <e> firme",
            "pub_date": "2026-10-29T08:00:00Z",
            "platforms": {
                "windows-x86_64": { "signature": "s", "url": "Postamon_0.2.0_x64-setup.exe" },
                "android-aarch64": { "signature": "s", "url": "Postamon_0.2.0.apk" }
            }
        })
    }

    #[test]
    fn finds_packages() {
        let m = manifest();
        let w = package(&m, "windows").unwrap();
        assert_eq!(w.file, "Postamon_0.2.0_x64-setup.exe");
        assert_eq!(w.date, "29/10/2026");
        assert_eq!(package(&m, "android").unwrap().file, "Postamon_0.2.0.apk");
        assert_eq!(package(&m, "mac"), None);
        assert_eq!(package(&Value::Null, "windows"), None);
        // Mai una cartella o un file nascosto
        let bad = json!({ "platforms": { "windows-x86_64": { "url": "../interno.sqlite" } } });
        assert_eq!(package(&bad, "windows"), None);
    }

    #[test]
    fn renders_page() {
        let html = render(&manifest(), "https://chat.lan:8790");
        assert!(html.contains(r#"href="/scarica/windows""#));
        assert!(html.contains(r#"href="/scarica/android""#));
        assert!(html.contains("Rubrica &lt;e&gt; firme"));
        assert!(html.contains("<svg") && html.contains("Inquadra con il telefono"));
        // Senza Android pubblicato: niente pulsante, "In arrivo"
        let only_windows = json!({ "version": "0.1.0", "platforms": { "windows-x86_64": { "url": "a.exe" } } });
        let html = render(&only_windows, "https://chat.lan:8790");
        assert!(html.contains("In arrivo") && !html.contains("/scarica/android"));
    }
}

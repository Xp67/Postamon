// Il server della chat proposto all'attivazione (e da cui si scaricano gli
// aggiornamenti finché nessuna casella è collegata) non sta nel codice: viene
// da POSTAMON_SERVER_CHAT o da `serverChat` in postamon.local.json, fuori da
// git (modello in postamon.esempio.json). Senza, l'utente lo scrive a mano.
fn main() {
    let local = std::path::Path::new("../postamon.local.json");
    println!("cargo:rerun-if-env-changed=POSTAMON_SERVER_CHAT");
    if local.exists() {
        println!("cargo:rerun-if-changed={}", local.display());
    }
    let server = std::env::var("POSTAMON_SERVER_CHAT").ok().or_else(|| {
        let text = std::fs::read_to_string(local).ok()?;
        // Blocco Note e PowerShell 5 salvano con il BOM, che serde_json rifiuta.
        let config: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
        config["serverChat"].as_str().map(str::to_string)
    });
    println!("cargo:rustc-env=POSTAMON_SERVER_CHAT={}", server.unwrap_or_default().trim());
    tauri_build::build()
}

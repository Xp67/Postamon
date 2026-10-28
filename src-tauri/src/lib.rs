//! Postamon: client di posta IMAP/SMTP per le caselle aziendali.
//!
//! - `accounts`: caselle configurate (`accounts.json`) e password nella
//!   Gestione credenziali di Windows.
//! - `store`: archivio locale SQLite (cartelle, intestazioni, messaggi scaricati).
//! - `imap`: connessione, sincronizzazione e operazioni sul server di posta.
//! - `signatures`: elenco delle firme (HTML con immagini), import da Outlook.
//! - `smtp`: composizione e invio dei messaggi.
//! - `render`: lettura MIME e preparazione dell'HTML da mostrare, ripulito.
//! - `sync`: controllo periodico della posta in arrivo e notifiche.
//! - `craft`: anteprima degli allegati con le app ArtCraft e loro installazione.
//! - `interno`: chat interna con il server Postamon interno.
//! - `update`: aggiornamenti automatici dal server interno.
//! - `addons`: add-on installati dal catalogo firmato, serviti isolati.
//! - `mailto`: Postamon come programma per i link email.
//! - `claude`: gli strumenti di Postamon per Claude (MCP) e il pannello Claude.
//! - `commands`: i comandi chiamati dall'interfaccia.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tauri::{Emitter, Manager};

/// Esegue un'operazione IMAP sulla connessione della casella. Se l'operazione
/// fallisce o va oltre il tempo massimo la connessione viene scartata, così
/// la volta dopo se ne apre una nuova.
macro_rules! with_imap {
    ($state:expr, $acc:expr, |$c:ident| $body:expr) => {{
        let mut lease = $state.imap.lease(&$acc).await?;
        let result = {
            let $c: &mut $crate::imap::Conn = lease.conn();
            match tokio::time::timeout(std::time::Duration::from_secs(300), $body).await {
                Ok(r) => r,
                Err(_) => Err(anyhow::anyhow!("Il server di posta non risponde")),
            }
        };
        if result.is_err() {
            lease.poison();
        }
        result
    }};
}

mod accounts;
mod addons;
#[cfg(windows)]
mod chrome;
mod claude;
mod commands;
mod craft;
mod imap;
mod interno;
mod mailto;
mod render;
mod signatures;
mod smtp;
mod store;
mod sync;
mod update;

pub struct AppState {
    pub accounts: accounts::Accounts,
    pub signatures: signatures::Signatures,
    pub store: store::Store,
    pub imap: imap::Pool,
    pub craft: craft::Craft,
    pub interno: Arc<interno::Interno>,
    pub updates: update::Updates,
    pub addons: addons::Addons,
    pub claude: claude::Claude,
    /// Link email con cui è stato aperto Postamon, finché l'interfaccia non lo prende.
    pub mailto: Mutex<Option<mailto::Mailto>>,
    /// Cartella dove si scrivono gli allegati aperti con un altro programma.
    pub temp_dir: PathBuf,
}

/// `postamon.exe --mcp <cartella>`: il server MCP per Claude, senza finestra.
/// Restituisce il codice di uscita, o `None` se è un avvio normale.
pub fn run_mcp() -> Option<i32> {
    claude::bridge::from_args(std::env::args_os()).map(claude::bridge::run)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Un solo Postamon aperto: un secondo avvio porta in primo piano la
        // finestra che c'è già (due copie controllerebbero la posta due volte)
        // e le passa il link email con cui è stato lanciato, se c'è.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if let Some(link) = mailto::from_args(&args)
                && let Some(state) = app.try_state::<Arc<AppState>>()
            {
                *state.mailto.lock().unwrap_or_else(|p| p.into_inner()) = Some(link);
                let _ = app.emit("mailto", ());
            }
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            // POSTAMON_DATA_DIR serve per le prove con un server di test,
            // senza toccare i dati veri.
            let dir = match std::env::var_os("POSTAMON_DATA_DIR") {
                Some(d) => PathBuf::from(d),
                None => app.path().app_data_dir()?,
            };
            std::fs::create_dir_all(&dir)?;
            if cfg!(debug_assertions) && std::env::var_os("POSTAMON_DATA_DIR").is_none() {
                accounts::import_installed(&dir);
            }
            let accounts = accounts::Accounts::load(dir.join("accounts.json"))?;
            let (signatures, fresh) = signatures::Signatures::load(dir.join("signatures.json"))?;
            if fresh && let Err(e) = signatures::first_run(&signatures, &accounts) {
                eprintln!("firme di Outlook e delle versioni precedenti non importate: {e:#}");
            }
            let state = Arc::new(AppState {
                accounts,
                signatures,
                store: store::Store::open(&dir.join("posta.sqlite"))?,
                imap: imap::Pool::default(),
                craft: craft::Craft::start(dir.join("craft"))?,
                interno: Arc::new(interno::Interno::load(dir.join("interno.json"))),
                updates: update::Updates::default(),
                addons: addons::Addons::start(dir.join("addons"))?,
                claude: claude::Claude::start(&dir)?,
                mailto: Mutex::new(mailto::from_args(std::env::args().skip(1))),
                temp_dir: std::env::temp_dir().join("Postamon"),
            });
            app.manage(state.clone());
            #[cfg(windows)]
            if let Some(window) = app.get_webview_window("main") {
                chrome::paint(&window);
            }
            // Solo la versione compilata per l'uso: una copia di sviluppo
            // prenderebbe il posto di quella installata.
            #[cfg(all(windows, not(debug_assertions)))]
            if let Err(e) = mailto::register(&app.package_info().name, "Posta e chat interna") {
                eprintln!("registrazione per i link email non riuscita: {e:#}");
            }
            state.interno.start_all(app.handle());
            state.claude.serve(app.handle().clone(), state.clone());
            update::spawn(app.handle().clone(), state.clone());
            sync::spawn(app.handle().clone(), state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::accounts_list,
            commands::account_add,
            commands::account_remove,
            commands::account_set_signatures,
            commands::signatures_list,
            commands::signature_save,
            commands::signature_delete,
            commands::signature_clean,
            commands::signature_image,
            commands::signatures_outlook,
            commands::signatures_import_outlook,
            commands::folders_list,
            commands::sync,
            commands::messages_list,
            commands::messages_load_older,
            commands::message_open,
            commands::message_set_seen,
            commands::message_set_flagged,
            commands::message_delete,
            commands::attachment_save,
            commands::attachment_open,
            commands::sender_trust,
            commands::contacts_suggest,
            commands::address_book_list,
            commands::address_book_save,
            commands::address_book_company,
            commands::address_book_delete,
            commands::address_book_companies,
            commands::address_book_company_save,
            commands::files_info,
            commands::compose_from,
            commands::message_send,
            commands::craft_status,
            commands::craft_install,
            commands::attachment_preview,
            commands::preview_close,
            commands::attachment_open_native,
            commands::interno_status,
            commands::interno_login,
            commands::interno_logout,
            commands::interno_call,
            commands::interno_send,
            commands::interno_file_save,
            commands::interno_file_preview,
            commands::interno_file_open,
            commands::update_status,
            commands::update_install,
            commands::addons_installed,
            commands::addons_catalog,
            commands::addon_install,
            commands::addon_uninstall,
            commands::addon_set_enabled,
            commands::addon_dev_add,
            commands::addon_message,
            commands::addon_storage,
            commands::mailto_take,
            commands::mailto_parse,
            commands::mailto_settings,
            commands::claude_status,
            commands::claude_set_enabled,
            commands::claude_set_model,
            commands::claude_login,
            commands::claude_link_desktop,
            commands::claude_link_code,
            commands::claude_context,
            commands::claude_requests,
            commands::claude_answer,
            commands::claude_chat_send,
            commands::claude_chat_stop,
            commands::claude_chat_reset,
        ])
        .run(tauri::generate_context!())
        .expect("avvio di Postamon non riuscito");
}

#[cfg(test)]
mod e2e;

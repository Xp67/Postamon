//! Postamon: client di posta IMAP/SMTP per le caselle aziendali.
//!
//! - `accounts`: caselle configurate (`accounts.json`) e password nella
//!   Gestione credenziali di Windows.
//! - `store`: archivio locale SQLite (cartelle, intestazioni, messaggi scaricati).
//! - `imap`: connessione, sincronizzazione e operazioni sul server di posta.
//! - `smtp`: composizione e invio dei messaggi.
//! - `render`: lettura MIME e preparazione dell'HTML da mostrare, ripulito.
//! - `sync`: controllo periodico della posta in arrivo e notifiche.
//! - `craft`: anteprima degli allegati con le app ArtCraft e loro installazione.
//! - `interno`: chat interna con il server Postamon interno.
//! - `update`: aggiornamenti automatici dal server interno.
//! - `addons`: add-on installati dal catalogo firmato, serviti isolati.
//! - `commands`: i comandi chiamati dall'interfaccia.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::Manager;

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
mod commands;
mod craft;
mod imap;
mod interno;
mod render;
mod smtp;
mod store;
mod sync;
mod update;

pub struct AppState {
    pub accounts: accounts::Accounts,
    pub store: store::Store,
    pub imap: imap::Pool,
    pub craft: craft::Craft,
    pub interno: Arc<interno::Interno>,
    pub updates: update::Updates,
    pub addons: addons::Addons,
    /// Cartella dove si scrivono gli allegati aperti con un altro programma.
    pub temp_dir: PathBuf,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Un solo Postamon aperto: un secondo avvio porta in primo piano la
        // finestra che c'è già (due copie controllerebbero la posta due volte).
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
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
            let state = Arc::new(AppState {
                accounts: accounts::Accounts::load(dir.join("accounts.json"))?,
                store: store::Store::open(&dir.join("posta.sqlite"))?,
                imap: imap::Pool::default(),
                craft: craft::Craft::start(dir.join("craft"))?,
                interno: Arc::new(interno::Interno::load(dir.join("interno.json"))),
                updates: update::Updates::default(),
                addons: addons::Addons::start(dir.join("addons"))?,
                temp_dir: std::env::temp_dir().join("Postamon"),
            });
            app.manage(state.clone());
            state.interno.start_all(app.handle());
            update::spawn(app.handle().clone(), state.clone());
            sync::spawn(app.handle().clone(), state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::accounts_list,
            commands::account_add,
            commands::account_remove,
            commands::account_set_signature,
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
        ])
        .run(tauri::generate_context!())
        .expect("avvio di Postamon non riuscito");
}

#[cfg(test)]
mod e2e;

# Postamon per Android

Piano della versione per telefono. Stato: **fase 0 fatta** (28/10/2026), fase 1 da iniziare.

## Decisioni (28/10/2026)

- **Stesso codice del PC**: Tauri 2 compila anche per Android. Il motore Rust gira sul telefono (IMAP e SMTP
  direttamente con Aruba, archivio SQLite locale), l'interfaccia React è la stessa, con un'impaginazione per lo
  schermo piccolo.
- **Prima versione**: posta, chat interna, rubrica, add-on.
- **Fuori**: pannello Claude (Claude Code non esiste su Android) e anteprime ArtCraft (programmi per Windows): sul
  telefono gli allegati si aprono con le app di Android.
- **Notifiche ad app chiusa**: un controllo ogni 5 minuti.
- **Distribuzione**: APK dal server della chat, come gli aggiornamenti del PC. Si scarica dalla rete del negozio
  o in VPN.

## Fasi

### 0. Strumenti

- SDK Android (c'era, con Android Studio), strumenti a riga di comando, NDK 29.0.14206865, target Rust
  `aarch64-linux-android` (telefoni) e `x86_64-linux-android` (emulatore).
- Per provare serve un telefono con il debug USB attivo, oppure un emulatore (immagine di sistema da installare).
- Per compilare a mano: `ANDROID_NDK_HOME` all'NDK, `CC_aarch64_linux_android` e
  `CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER` a `aarch64-linux-android24-clang.cmd`, `AR_aarch64_linux_android` a
  `llvm-ar.exe` (tutti in `toolchains/llvm/prebuilt/windows-x86_64/bin` dell'NDK).

**Prova di compilazione** (28/10, su una copia, `cargo check --lib --target aarch64-linux-android`): con
`native-tls`, `keyring`, `tauri-plugin-updater` e `tauri-plugin-single-instance` solo per il PC, e lettre su
rustls (`tokio1-rustls`, `ring`, `webpki-roots`), **tutte le dipendenze compilano**, anche quelle in C (SQLite,
ring). Restano 17 errori nel codice di Postamon, tutti dove previsto: `imap.rs` (TLS), `accounts.rs` e
`interno.rs` (keyring), `lib.rs` e `update.rs` (plugin del PC), `claude/mod.rs` (metodi della finestra del PC).

### 1. Il motore compila per Android

- **TLS**: oggi IMAP e SMTP usano `native-tls`, che su Android vorrebbe OpenSSL compilato per il telefono. Su
  Android si passa a rustls (`tokio-rustls` per IMAP, `tokio1-rustls` per lettre), con i certificati di sistema.
- **Password**: `keyring` non ha un archivio per Android. Serve un piccolo plugin Kotlin con il Keystore di
  Android, dietro le stesse funzioni di `accounts.rs` (`password`, `set_password`, `delete_password`).
- **Solo PC** (`#[cfg(desktop)]`): aggiornamenti (`tauri-plugin-updater`), istanza unica, link email
  (`mailto.rs`), ArtCraft (`craft.rs`), Claude (`claude/`), cornice della finestra (`chrome.rs`), firme di Outlook.
- Cartella dati: `app_data_dir()` funziona anche su Android.

### 2. Interfaccia per il telefono

- Una colonna alla volta: caselle (menu laterale) → cartelle → elenco → messaggio, con il tasto indietro di
  Android. Scrittura a schermo intero. Chat, rubrica e add-on con la stessa logica.
- Stesso codice React, con l'impaginazione scelta dalla larghezza dello schermo (e un'indicazione della
  piattaforma per ciò che sul telefono non c'è).
- Allegati: si aprono con le app del telefono o si salvano in Download.
- Add-on: il server locale su 127.0.0.1 resta; la WebView di Android va autorizzata al traffico in chiaro solo
  verso 127.0.0.1 (network security config).

### 3. Notifiche ogni 5 minuti

- Ad app aperta: IDLE come sul PC (notifica subito).
- Ad app chiusa: una sveglia di sistema (`AlarmManager.setAndAllowWhileIdle`) ogni 5 minuti, rimessa anche
  all'accensione del telefono. La sveglia chiama il motore Rust (funzione esportata via JNI) che controlla le
  caselle e restituisce i messaggi nuovi; Kotlin mostra le notifiche.
- Limite di Android: con il telefono fermo a lungo (Doze) le sveglie si diradano fino a circa una ogni 10
  minuti. Il servizio periodico standard (WorkManager) non scende sotto i 15 minuti. Per avere davvero sempre 5
  minuti servirebbe un servizio sempre attivo, con l'icona fissa nella barra.
- Permesso di notifica (Android 13+) chiesto al primo avvio.

### 4. Distribuzione

- Chiave di firma dell'APK nuova, da custodire come quella degli aggiornamenti del PC (se si perde, gli APK
  nuovi non si installano sopra i vecchi).
- `latest.json` sul server della chat ha già una mappa di piattaforme: accanto a `windows-x86_64` si aggiunge
  `android-aarch64` con il nome dell'APK e la firma minisign (stessa chiave degli aggiornamenti). Il server non
  cambia: serve già qualunque file con un nome semplice da `dati/aggiornamenti/`.
- Nell'app: controllo come sul PC, scaricamento, verifica della firma (`minisign-verify`), poi l'installer di
  Android (FileProvider con una classe propria, vedi la lezione dell'app di c4shopper).
- `pm ship` compila anche l'APK (`tauri android build --apk`) e lo carica con lo stesso numero di versione.

### 5. Chat interna sul telefono

- Funziona quando il telefono raggiunge il server (Wi-Fi del negozio o VPN). WebSocket ad app aperta; ad app
  chiusa il controllo dei 5 minuti può guardare anche i messaggi della chat.

## Da sapere

- Da gennaio 2027 Google chiede uno sviluppatore verificato anche per le app installate fuori dal Play Store;
  l'account gratuito "distribuzione limitata" copre fino a 20 dispositivi.

# Postamon

Client di posta per Windows, scritto in Rust (Tauri 2) con l'interfaccia in React: riceve e invia la posta
delle caselle aziendali, senza il resto di Outlook.

## Cosa fa (fase 1)

- **Una casella, uno spazio di lavoro.** La colonna a sinistra ha un'icona per casella (con i messaggi da leggere,
  posta più chat) e il "+" per aggiungerne. Ogni spazio mostra solo le cartelle, la posta e la chat interna di
  quella casella; i nuovi messaggi partono da lì. Postamon ricorda dove si era (cartella o chat) in ogni spazio.
  Le caselle non aperte continuano a ricevere posta e notifiche.
- **Ricezione IMAP**: alla prima apertura scarica le intestazioni degli ultimi 300 messaggi di una cartella,
  poi solo le novità; "Carica messaggi precedenti" va indietro di 300 alla volta. I messaggi recenti sotto i
  2 MB si scaricano in anticipo, per avere anteprima e apertura immediata.
- **Controllo automatico** della posta in arrivo ogni minuto, con notifica di Windows per i messaggi nuovi.
- **Lettura delle email impaginate**: l'HTML viene ripulito (niente script né moduli), le immagini da internet
  restano bloccate finché non le chiedi ("Mostra immagini" o "Sempre da questo mittente"). Il messaggio è
  mostrato in un riquadro isolato con una Content-Security-Policy; i link si aprono nel browser.
- **Allegati**: si aprono con il programma predefinito o si salvano. I tipi eseguibili (.exe, .js, .bat, .lnk…)
  non si aprono da qui: vanno salvati.
- **Scrittura**: nuovo messaggio, rispondi, rispondi a tutti, inoltra (con gli allegati originali), Cc e Ccn,
  allegati da pulsante o trascinandoli sulla finestra, suggerimento degli indirizzi già visti, firma per casella
  (alla prima configurazione viene copiata da Outlook se c'è). Dopo l'invio la copia va in "Posta inviata".
- **Password** nella Gestione credenziali di Windows, mai su file.

Scorciatoie: `Ctrl+N` nuovo messaggio, `F5` aggiorna, frecce su/giù nell'elenco, `Canc` elimina,
`Ctrl+Invio` invia, `Esc` chiude la finestra di scrittura.

## Installazione e aggiornamenti

**Installer**: `Postamon_<versione>_x64-setup.exe`, in italiano, per l'utente che lo lancia (in
`%LOCALAPPDATA%\Postamon`, senza diritti di amministratore), con la voce nel menu Start. I dati restano in
`%APPDATA%\it.dimensionesalotto.postamon`, quindi reinstallare o aggiornare non tocca caselle né posta. Si apre un
solo Postamon alla volta: un secondo avvio porta in primo piano la finestra che c'è già. L'installer non è firmato
con un certificato di sviluppatore: se arriva da internet Windows mostra "PC protetto" (Ulteriori informazioni →
Esegui comunque); copiato dalla rete o da una chiavetta di solito no.

**Aggiornamenti automatici**: Postamon controlla 30 secondi dopo l'avvio e poi ogni 4 ore
`https://<server della chat>/api/aggiornamenti/latest.json` (il server della chat di una delle caselle, altrimenti
quello predefinito della compilazione, vedi "Configurazione locale"). Se c'è una versione nuova la scarica in sottofondo e in alto compare "Postamon x.y.z è pronto"
con **Riavvia e aggiorna**: l'installer gira con una piccola barra di avanzamento e riapre Postamon aggiornato.
Ogni pacchetto è firmato (minisign, versione compresa) con la chiave in `~\.tauri\postamon.key`: un pacchetto
senza quella firma non si installa. La chiave pubblica è in `src-tauri/tauri.conf.json`.

> **La chiave privata `~\.tauri\postamon.key` va conservata in un posto sicuro (copia fuori da questo PC).** Senza
> quella non si possono più pubblicare aggiornamenti: i Postamon installati andrebbero reinstallati a mano con una
> versione che porta una chiave nuova.

Pubblicare una versione (versione nei tre file, installer firmato, invio al server):

```powershell
powershell -ExecutionPolicy Bypass -File scripts\pubblica.ps1 -Versione 0.2.0 -Note "Cosa cambia"
```

Compila in `src-tauri\target\dist` (non tocca un Postamon aperto da `target\release`); l'installer da dare ai
colleghi è `src-tauri\target\dist\release\bundle\nsis\Postamon_<versione>_x64-setup.exe`. Sul server i file
stanno in `~/postamon-interno/dati/aggiornamenti/`; `latest.json` si carica per ultimo e porta solo il nome
dell'installer, che il server completa con il proprio indirizzo. `-SenzaInvio` si ferma dopo la compilazione.

### Configurazione locale

Gli indirizzi della rete aziendale non stanno nella repo: vanno in `postamon.local.json` nella radice, escluso da
git (modello in `postamon.esempio.json`):

```json
{
  "serverChat": "chat.azienda.lan:8790",
  "utenteSsh": "utente",
  "chiaveSsh": "~\\.ssh\\id_ed25519"
}
```

- `serverChat`: server della chat proposto all'attivazione e usato per gli aggiornamenti finché nessuna casella è
  collegata. Lo legge `src-tauri/build.rs` alla compilazione (la variabile `POSTAMON_SERVER_CHAT` ha la
  precedenza); senza, l'utente lo scrive nella schermata "Attiva la chat".
- `utenteSsh`, `chiaveSsh`: accesso al server per `scripts\pubblica.ps1` (sovrascrivibili con `-Server`, `-Utente`,
  `-ChiaveSsh`). Il server è l'host di `serverChat`.

## Server delle caselle aziendali

Aruba con i nomi del gestore, accesso con indirizzo email e password (già proposti alla configurazione):

| | Server | Porta |
|---|---|---|
| Ricezione (IMAP, SSL) | `in.postassl.it` | 993 |
| Invio (SMTP, SSL) | `out.postassl.it` | 465 |

Sull'SMTP la porta 587 (e la 25) usa STARTTLS, tutte le altre la connessione cifrata da subito.

## Sviluppo

```sh
npm install
npm run tauri dev          # app con ricarica automatica
npm run check              # controllo dei tipi TypeScript
cd src-tauri && cargo test # test del motore
npx tauri build --no-bundle  # src-tauri/target/release/postamon.exe (senza installer né firma)
```

`POSTAMON_DATA_DIR=<cartella>` fa usare all'app una cartella dati diversa da quella vera
(`%APPDATA%\it.dimensionesalotto.postamon`), utile per le prove.

### Prova completa con un server di posta di test

```sh
docker run -d --name postamon-greenmail -p 3993:3993 -p 3465:3465 \
  -e GREENMAIL_OPTS="-Dgreenmail.setup.test.all -Dgreenmail.hostname=0.0.0.0 -Dgreenmail.users=marco:segreta@localhost -Dgreenmail.users.login=email" \
  greenmail/standalone
cd src-tauri
POSTAMON_E2E=1 cargo test e2e                       # invio, sincronizzazione, apertura, flag, inviata, cestino
POSTAMON_SEED=<cartella> cargo test seed_demo       # cartella dati con messaggi di esempio
POSTAMON_DATA_DIR=<cartella> npm run tauri dev      # (dalla radice) l'app su quei dati
```

## Struttura

| File | Contenuto |
|---|---|
| `src-tauri/src/imap.rs` | connessione, cartelle, sincronizzazione, flag, cestino, copia in inviata |
| `src-tauri/src/smtp.rs` | composizione MIME e invio |
| `src-tauri/src/render.rs` | lettura MIME, pulizia dell'HTML, testo per anteprime e citazioni |
| `src-tauri/src/store.rs` | archivio locale SQLite |
| `src-tauri/src/accounts.rs` | caselle e password |
| `src-tauri/src/sync.rs` | controllo periodico e notifiche |
| `src-tauri/src/update.rs` | aggiornamenti automatici dal server della chat |
| `scripts/pubblica.ps1` | installer firmato e pubblicazione di una versione |
| `src-tauri/src/commands.rs` | comandi chiamati dall'interfaccia |
| `src/` | interfaccia React |

## Chat interna

"Chat interna" nella barra laterale: chat a due e di gruppo fra colleghi, messaggi istantanei e allegati di
qualunque dimensione che restano nella rete aziendale. Ogni casella ha la sua chat (il suo spazio di lavoro):
l'account è l'indirizzo email e si attiva con un clic, perché la password, che il server verifica su Aruba,
Postamon la conosce già. Le sessioni delle caselle restano collegate insieme in sottofondo (`interno.json`, un
token per casella nella Gestione credenziali). Chi è spento riceve tutto alla riaccensione. Gli allegati ricevuti si aprono con le app ArtCraft come quelli
della posta.

Il server è `server/` (crate `postamon-interno`, Rust: axum, SQLite, LDAP):

- **Certificato**: autofirmato, generato alla prima partenza in `dati/`; Postamon ne fissa l'impronta SHA-256 al
  primo accesso e poi rifiuta qualunque altro certificato.
- **Accesso** (`[imap]` nella configurazione): email e password si verificano con un accesso IMAP su TLS a
  `in.postassl.it` (AUTHENTICATE PLAIN, poi LOGOUT); la password non si conserva. Sono ammessi solo gli indirizzi
  dei domini in `allowed_domains`. I colleghi compaiono in rubrica dal loro primo accesso. Il codice supporta anche
  l'accesso con l'account del dominio (`[ldap]`), oggi non usato.
- **Allegati**: caricati a blocchi da 4 MB (`PUT /api/uploads/{id}?offset=`) e scaricati con `Range`: se la
  connessione cade si riprende da dove si era arrivati.
- **Tempo reale**: WebSocket `/api/ws`; Postamon si ricollega da solo e mostra una notifica di Windows quando non
  è in primo piano.
- **Aggiornamenti di Postamon**: `GET /api/aggiornamenti/latest.json` e `/api/aggiornamenti/<installer>`, senza
  accesso, dalla cartella `dati/aggiornamenti/` (vedi "Installazione e aggiornamenti"). Senza `latest.json`
  risponde 204, cioè "niente di nuovo".

**Installazione** (Linux), cartella `~/postamon-interno/` dell'utente che lo fa girare: eseguibile,
`postamon-interno.toml` (modello in `server/deploy/postamon-interno.esempio.toml`), `dati/`, `interno.log`. Lo
tengono acceso `avvia.sh` (nel crontab dell'utente: `@reboot` e ogni 5 minuti) e `riavvia.sh` dopo un
aggiornamento. Gli utenti di prova facoltativi (`[[local_users]]`) hanno le password solo nel file di
configurazione sul server.

Aggiornare il server (`<utente>@<server>` e la chiave come in `postamon.local.json`):

```sh
cd server
cargo test
cargo zigbuild --release --target x86_64-unknown-linux-musl   # serve zig nel PATH (pip install ziglang)
scp -i <chiave> target/x86_64-unknown-linux-musl/release/postamon-interno <utente>@<server>:postamon-interno/postamon-interno.nuovo
ssh -i <chiave> <utente>@<server> 'cd postamon-interno && mv postamon-interno.nuovo postamon-interno && chmod +x postamon-interno && ./riavvia.sh'
```

`scp` da Windows non porta il permesso di esecuzione: senza `chmod +x` il server non riparte.

Prova del client contro il server vero, con due utenti di prova: `POSTAMON_INTERNO_SERVER=<server>
POSTAMON_INTERNO_U1=<utente 1> POSTAMON_INTERNO_P1=… POSTAMON_INTERNO_U2=<utente 2> POSTAMON_INTERNO_P2=…
cargo test live_server_round_trip` (in `src-tauri`).

## Prossime fasi

- Chat interna in uso: spostare il server su una macchina dedicata vicino al controller di dominio.
- Posta: conversazioni, bozze, ricerca sul server.

## ArtCraft

Postamon si appoggia alle app [ArtCraft](https://getartcraft.com) (github.com/storytold, MIT/Apache-2.0),
`src-tauri/src/craft.rs`:

- **Installazione.** All'avvio, se sul PC mancano programmi ArtCraft, Postamon propone di installarli
  ("caldamente raccomandato per l'utilizzo corretto di Postamon"). Scarica gli MSI ufficiali dalle release
  GitHub, ne controlla lo SHA-256 con `SHA256SUMS.txt` e li installa in silenzio con
  `INSTALLDESKTOPSHORTCUT=0`, cioè senza icone sul desktop: il launcher per l'utente, le app (in Programmi)
  tutte insieme con una sola conferma di amministratore.
- **Anteprima dentro Postamon.** Un clic su un allegato lo apre in un pannello con l'app adatta, più
  "Apri in <app>" (l'app installata, a finestra piena) e "Salva". Ogni app ha una versione web pensata per stare
  in un iframe: Postamon la scarica la prima volta (stesso controllo SHA-256, avanzamento a schermo) e la serve
  con l'allegato da un server su `127.0.0.1`, un'origine diversa dall'interfaccia. PdfCraft apre il file con
  `?file=`; le altre lo ricevono come un file trascinato. I tipi che nessuna app apre vanno al programma
  predefinito di Windows. Provato nell'app con PdfCraft (PDF) e PhotoCraft (JPEG).

| Tipo di allegato | App |
|---|---|
| PDF | PdfCraft |
| immagini (jpg, png, heic, psd, tiff…) | PhotoCraft |
| docx, doc, odt, rtf, md, txt | WordCraft |
| xlsx, xls, ods, csv | GridCraft |
| pptx | DeckCraft |
| svg, ai, eps | VectorCraft |
| wav, mp3, flac… | SoundCraft |
| mp4, mov, webm… | FilmCraft |
| dxf, dwg | CADCraft |

Prova nell'app senza server di posta: `POSTAMON_SEED_OFFLINE=<cartella> cargo test seed_offline` crea una
cartella dati con due messaggi (un PDF e una foto); poi `POSTAMON_DATA_DIR=<cartella>` sull'eseguibile.

Banco di prova delle anteprime nel browser:
`POSTAMON_CRAFT_SERVE=<secondi> POSTAMON_CRAFT_ROOT=<cartella> cargo test craft_serve -- --nocapture`
(gli indirizzi da aprire nel browser finiscono in `<cartella>/urls.json`).

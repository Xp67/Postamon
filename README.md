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
  allegati da pulsante o trascinandoli sulla finestra, suggerimento degli indirizzi già visti. Dopo l'invio la
  copia va in "Posta inviata".
- **Firme** (menu della casella → Firme): un elenco di firme con nome, come in Outlook, scritte in un editor
  visuale (carattere, dimensione, colori, link, immagini dal PC o incollate, colonne logo + recapiti, linea) o
  direttamente in HTML. Per ogni casella si sceglie la firma dei messaggi nuovi e quella di risposte e inoltri;
  mentre scrivi la firma compare sotto il testo, già impaginata, e si può cambiare o togliere per quel messaggio.
  Nelle risposte e negli inoltri sta fra il testo e il messaggio originale. Le immagini partono dentro il
  messaggio (parti `cid:`), non come link. Alla prima partenza Postamon importa le firme di Outlook del PC, con
  le loro immagini, e dà a ogni casella quella col suo indirizzo nel nome; "Importa da Outlook" prende quelle
  aggiunte dopo.
- **Rubrica per casella**: accanto al nome di un mittente che non è in rubrica c'è un'icona; la finestrella
  che apre (nasce dall'icona e ci rientra chiudendosi) ha già il nome del mittente e la sua email, più azienda,
  telefono e note facoltativi. Le mail dei contatti in rubrica hanno a sinistra un semicerchio verde e il nome
  scritto in rubrica; nella mail aperta l'etichetta "In rubrica" porta al contatto. La vista **Rubrica** (sotto
  la chat) mostra i contatti in ordine alfabetico o raggruppati per dominio, con ricerca, "Nuovo contatto",
  modifica, "Scrivi" e "Togli dalla rubrica". Un dominio aziendale vale come azienda: a un contatto nuovo si
  propone l'azienda dei colleghi con lo stesso dominio, e quando si scrive o si rinomina l'azienda di un contatto
  la prendono anche i colleghi senza azienda o con il vecchio nome (chi ne ha una diversa resta com'è). I domini
  di uso comune (Gmail, Outlook, Libero, PEC…) non contano: l'elenco è `PUBLIC_DOMAINS` in `store.rs`.
  Nella vista per dominio (`src/book.ts`) chi scrive da un dominio comune ma è di un'azienda con un dominio
  aziendale sta nel gruppo di quel dominio; un'azienda con più domini diventa un gruppo con un sottogruppo per
  dominio più "Altri indirizzi". Ogni azienda ha una **scheda** (Partita IVA, telefono, indirizzo, sito, note,
  persone, "Scrivi a tutti"), aperta dall'intestazione del gruppo o dall'azienda nella scheda di una persona;
  rinominandola cambia il nome a tutte le sue persone, e se la si rinomina dalla scheda di una persona la scheda
  la segue. Il campo "Azienda" propone le aziende già in rubrica. Togliendo la casella da Postamon la rubrica
  resta nell'archivio e torna se la si riaggiunge. Le animazioni usano anime.js (`src/anim.ts`), che con "meno movimento" di Windows
  porta subito allo stato finale.
- **Barra del titolo nel colore dell'app** (Windows 11): la barra di Windows prende il grigio-azzurro della
  fascia in alto, così a finestra ingrandita non c'è stacco; pulsanti, aggancio ai bordi e doppio clic restano
  quelli di sistema. Su Windows 10 resta la barra normale.
- **Password** nella Gestione credenziali di Windows, mai su file.
- **Link email** (`mailto:`): a ogni avvio la versione installata si registra, per l'utente, fra i programmi che
  aprono i link email, così Windows la propone quando se ne clicca uno (in un sito, in un PDF, in un altro
  programma). Il link diventa un nuovo messaggio nello spazio di lavoro aperto, con destinatari, Cc, Ccn, oggetto
  e testo; gli allegati che qualche link prova a indicare (`attach=`) no. Se Postamon è già aperto il link arriva
  lì; se si sta scrivendo un altro messaggio, aspetta che lo si chiuda. Windows non lascia che un programma si
  renda predefinito da solo: dal menu della casella, **Apri i link email con Postamon** apre la pagina di
  Impostazioni dove sceglierlo. La disinstallazione toglie la registrazione (`src-tauri/windows/hooks.nsh`); le
  copie di sviluppo (`tauri dev`) non si registrano, per non prendere il posto di quella installata.

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

### Pubblicare: `pm ship`

`scripts/pm` (bash, per Git Bash, sul modello di `c4` di c4shopper) porta a tutti quello che c'è in `main`. La
prova si fa prima, in locale, con Postamon Dev (vedi "Sviluppo"):

```sh
pm status                  # versione nel codice e pubblicata, commit da pubblicare, server della chat
pm ship                    # chiede versione (patch/minor/major) e nota per i colleghi
pm ship minor -m "Rubrica e pannello Claude"
pm server                  # solo il server della chat
pm rollback server         # rimette il server della chat di prima
pm rollback app            # il server torna a proporre la versione di prima
```

`pm ship` si ferma prima di toccare qualsiasi cosa se non si è su `main`, ci sono modifiche non committate, `main` è
indietro rispetto a GitHub, il server non si raggiunge o i test (`npm run check`, `cargo test` dell'app e del
server) non passano. Poi controlla, nell'ordine, le due cose che poi aggiorna:

1. **Server della chat**: va aggiornato se `server/` è cambiato dal commit da cui è compilato (lo tiene in
   `~/postamon-interno/commit`), se quel commit non si sa o se il server non risponde. Viene prima perché l'app
   nuova può averne bisogno, ed è lui a distribuirla.
2. **App**: sul server deve esserci l'ultima versione. Se ci sono commit dell'app dopo l'ultimo tag `v*` serve una
   versione nuova; se l'ultimo tag non è mai arrivato sul server (un ship interrotto, un invio fallito) va
   ripubblicato, con l'installer già compilato o ricompilandolo uguale. Se l'app non è cambiata non si fa una
   versione nuova: i colleghi reinstallerebbero la stessa.

Mostra cosa farà e, con la conferma, pusha `main` e poi:

1. server: lo compila per Linux, lo carica, lo riavvia e controlla che risponda; se non risponde rimette da solo
   quello di prima (`postamon-interno.prima`);
2. app: scrive la versione, compila l'installer firmato, fa il commit `Postamon x.y.z` e il tag `vx.y.z`, li pusha
   e carica installer e `latest.json` sul server.

`pm rollback app` non riporta indietro chi si è già aggiornato (Postamon non torna a una versione più vecchia):
ferma la versione per gli altri. Per correggerla si pubblica una versione nuova.

Il motore della compilazione è `scripts\pubblica.ps1`, che si può usare anche da solo:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\pubblica.ps1 -Versione 0.2.0 -Note "Cosa cambia"
```

Scrive la versione (`tauri.conf.json`, `Cargo.toml`, `package.json`, `package-lock.json`) e compila in
`src-tauri\target\dist` (non tocca un Postamon aperto da `target\release`); l'installer da dare ai colleghi è
`src-tauri\target\dist\release\bundle\nsis\Postamon_<versione>_x64-setup.exe`. Sul server i file stanno in
`~/postamon-interno/dati/aggiornamenti/`; `latest.json` si carica per ultimo e porta solo il nome
dell'installer, che il server completa con il proprio indirizzo, e quello di prima resta in `latest.prima.json`.
`-SenzaInvio` si ferma dopo la compilazione, `-SoloInvio` carica una versione già compilata.

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
- `utenteSsh`, `chiaveSsh`: accesso al server per `scripts/pm` e `scripts\pubblica.ps1` (in questo sovrascrivibili
  con `-Server`, `-Utente`, `-ChiaveSsh`). Il server è l'host di `serverChat`.

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
pm dev                     # Postamon Dev, con ricarica automatica (uguale a: npm run app)
npm run check              # controllo dei tipi TypeScript
cd src-tauri && cargo test # test del motore
npx tauri build --no-bundle  # src-tauri/target/release/postamon.exe (senza installer né firma)
```

**Postamon Dev** è l'app di sviluppo: `tauri dev` con `src-tauri/tauri.dev.conf.json`. Ha lo stesso logo in arancio e
viola (`src-tauri/icons-dev/`), il titolo "Postamon Dev" e un identificativo suo
(`it.dimensionesalotto.postamon.dev`), quindi gira accanto al Postamon installato senza toccarlo:

- cartella dati propria, `%APPDATA%\it.dimensionesalotto.postamon.dev`. La prima volta parte con le caselle del
  Postamon installato (copia il suo `accounts.json`: l'elenco e le firme, non la posta, che riscarica);
- le compilazioni di sviluppo tengono password e accesso alla chat sotto altri nomi nella Gestione credenziali
  (`Postamon Dev`, `Postamon Dev-interno`). Finché una casella non ha una password sua, usano quella del Postamon
  installato, solo in lettura: togliere o cambiare una casella in Postamon Dev non tocca l'installato;
- si collegano a Claude come `postamon-dev`, non si aggiornano da sole e non si registrano per i link email.

L'interfaccia si aggiorna a ogni salvataggio senza riavviare; una modifica al Rust ricompila e riapre la finestra.
`npm run tauri dev` senza la configurazione di sviluppo avrebbe l'identificativo del Postamon installato: se quello
è aperto, si limita a portarlo in primo piano.

`POSTAMON_DATA_DIR=<cartella>` fa usare all'app una cartella dati diversa, utile per le prove.

### Prova completa con un server di posta di test

```sh
docker run -d --name postamon-greenmail -p 3993:3993 -p 3465:3465 \
  -e GREENMAIL_OPTS="-Dgreenmail.setup.test.all -Dgreenmail.hostname=0.0.0.0 -Dgreenmail.users=marco:segreta@localhost -Dgreenmail.users.login=email" \
  greenmail/standalone
cd src-tauri
POSTAMON_E2E=1 cargo test e2e                       # invio, sincronizzazione, apertura, flag, inviata, cestino
POSTAMON_SEED=<cartella> cargo test seed_demo       # cartella dati con messaggi di esempio
POSTAMON_DATA_DIR=<cartella> npm run app            # (dalla radice) Postamon Dev su quei dati
```

## Struttura

| File | Contenuto |
|---|---|
| `src-tauri/src/imap.rs` | connessione, cartelle, sincronizzazione, flag, cestino, copia in inviata |
| `src-tauri/src/smtp.rs` | composizione MIME e invio |
| `src-tauri/src/signatures.rs` | elenco delle firme, pulizia dell'HTML, import da Outlook, immagini come `cid:` |
| `src-tauri/src/render.rs` | lettura MIME, pulizia dell'HTML, testo per anteprime e citazioni |
| `src-tauri/src/store.rs` | archivio locale SQLite, rubrica compresa |
| `src-tauri/src/accounts.rs` | caselle e password |
| `src-tauri/src/sync.rs` | controllo periodico e notifiche |
| `src-tauri/src/update.rs` | aggiornamenti automatici dal server della chat |
| `src-tauri/src/mailto.rs` | link email: lettura del link e registrazione in Windows |
| `src-tauri/src/chrome.rs` | colore della barra del titolo di Windows |
| `src-tauri/src/addons.rs` | add-on: catalogo firmato, installazione, server locale isolato, dati |
| `src-tauri/src/claude/` | Claude: server MCP (`--mcp`), strumenti, conferme, Claude Code e pannello |
| `scripts/pm` | sviluppo e pubblicazione da terminale: `pm dev`, `pm status`, `pm ship`, `pm rollback` |
| `scripts/pubblica.ps1` | installer firmato e pubblicazione di una versione (lo usa `pm ship`) |
| `src-tauri/tauri.dev.conf.json`, `src-tauri/icons-dev/` | Postamon Dev: nome, identificativo e logo dell'app di sviluppo |
| `src-tauri/src/commands.rs` | comandi chiamati dall'interfaccia |
| `src/` | interfaccia React |
| `addons/` | sorgenti degli add-on (non entrano nella build) |
| `sdk/` | libreria e stile serviti agli add-on |
| `scripts/addons.mjs`, `scripts/pubblica-addon.ps1` | controllo, pacchetti e pubblicazione del catalogo degli add-on |

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

Aggiornare il server: `pm server` (o `pm ship`, che lo aggiorna quando `server/` è cambiato) compila, carica,
riavvia, controlla che risponda e tiene il server di prima per `pm rollback server`. A mano
(`<utente>@<server>` e la chiave come in `postamon.local.json`):

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

## Add-on

Funzioni in più che ognuno installa solo se gli servono, dal **marketplace** (icona del puzzle in basso nella
colonna delle caselle): un pulsante sul messaggio aperto, una vista nella barra laterale. Chi vuole scriverne uno
segue [ADDONS.md](ADDONS.md) e propone una pull request; come contribuire in generale: [CONTRIBUTING.md](CONTRIBUTING.md).

- **Il codice resta nel repo** (`addons/<id>/`) e non entra nella build. Dopo il merge,
  `scripts\pubblica-addon.ps1` lo impacchetta nel catalogo (release `addons` del repo su GitHub), firmato con
  `~\.tauri\postamon-addons.key`, una chiave diversa da quella degli aggiornamenti.
- **Installazione**: Postamon verifica la firma del catalogo e lo SHA-256 del pacchetto, mostra i permessi e
  installa in `%APPDATA%\it.dimensionesalotto.postamon\addons\<id>\`. Disattivare nasconde l'add-on, disinstallare
  lo cancella con i suoi dati.
- **Isolamento**: le pagine si servono da un server su 127.0.0.1 in un riquadro senza origine propria
  (`sandbox="allow-scripts allow-forms"`), con una Content-Security-Policy che vieta ogni collegamento esterno
  salvo i domini dichiarati con `network:`. L'add-on parla con Postamon solo con `postMessage`
  (`sdk/postamon-addon.js`) e ottiene solo ciò che i permessi concedono; la pagina principale impedisce ai
  riquadri di navigare fuori da 127.0.0.1. Provato in WebView2: rete, interfaccia, localStorage e comandi di
  Tauri non sono raggiungibili dal riquadro.
- **Controllo automatico**: `node scripts/addons.mjs check` (lo esegue GitHub su ogni pull request, con le regole
  prese da `main`).

`POSTAMON_ADDONS_URL=<cartella con addons.json>/` fa leggere a Postamon un catalogo diverso da quello pubblicato,
per le prove (`scripts\pubblica-addon.ps1 -SenzaInvio` lo prepara in `dist-addons\`).

## Claude

Postamon usa Claude con l'**abbonamento dell'utente**, senza mai vedere l'account. Anthropic non permette alle app
di terzi di offrire l'accesso con Claude né di usare i piani Free/Pro/Max al posto dell'utente
([condizioni](https://code.claude.com/docs/en/legal-and-compliance)). L'abbonamento lo usano i programmi di
Anthropic, e Postamon dà loro i suoi strumenti con il Model Context Protocol (MCP), `src-tauri/src/claude/`:

- **Pannello «Claude»** (pulsante in alto a destra): avvia il Claude Code installato sul PC, così com'è, con
  `claude -p` in streaming. Ha solo gli strumenti di Postamon: niente comandi, file o internet, niente impostazioni
  né altri server MCP dell'utente. «Accedi» apre `claude auth login`. Il modello si sceglie in alto a destra nel
  pannello (Haiku di base): cambiandolo, la conversazione continua con il nuovo modello.
- **Claude Desktop e Claude Code**: «Collega», nelle impostazioni del pannello, aggiunge Postamon ai loro server MCP
  (`claude_desktop_config.json`, con una copia di sicurezza, e `claude mcp add --scope user`). Avviano
  `postamon.exe --mcp <cartella dei dati>`, che risponde da solo a `initialize` e `tools/list` e passa le chiamate
  al Postamon aperto, aprendolo se è chiuso. Il passaggio avviene su un server locale con token (`claude/mcp.json`,
  rinnovato a ogni avvio).
- **Strumenti** (`tools.rs`): caselle, cartelle, elenco e ricerca nell'archivio, lettura di un messaggio (senza
  segnarlo letto), messaggio aperto, bozza nella finestra di scrittura, invio, segna, sposta, cestina, add-on
  (elenco, installa, togli, attiva) e «mostra in Postamon».
- **Conferme** (`confirm.rs`): il testo delle email arriva a Claude e può contenere istruzioni di chiunque.
  Per questo inviare, spostare, cestinare e installare o togliere add-on si confermano in Postamon, in una finestra
  che mostra tutto (per un invio anche «Modifica»). Lo strumento aspetta 45 s; se nessuno risponde, restituisce il
  numero della richiesta, da controllare con `request_status`. Il collegamento resta spento finché l'utente non
  lo accende.
- **Prima di distribuire il pannello**: Anthropic chiede a chi esegue Claude Code dentro un proprio prodotto di
  accettare i [Commercial Terms](https://www.anthropic.com/legal/commercial-terms). Ogni utente entra con il
  proprio account Claude.

`POSTAMON_CLAUDE_DESKTOP=<file>` fa usare a Postamon un file di configurazione di Claude Desktop di prova al posto
di quello vero.

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

Prova nell'app senza server di posta (con cinque contatti in rubrica, uno è il mittente di una mail):
`POSTAMON_SEED_OFFLINE=<cartella> cargo test seed_offline` crea una
cartella dati con due messaggi (un PDF e una foto); poi `POSTAMON_DATA_DIR=<cartella>` sull'eseguibile.

Banco di prova delle anteprime nel browser:
`POSTAMON_CRAFT_SERVE=<secondi> POSTAMON_CRAFT_ROOT=<cartella> cargo test craft_serve -- --nocapture`
(gli indirizzi da aprire nel browser finiscono in `<cartella>/urls.json`).

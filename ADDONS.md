# Scrivere un add-on per Postamon

Un add-on è una piccola app web (HTML, CSS, JavaScript) che aggiunge una funzione a Postamon: un pulsante sul
messaggio aperto, una vista nella barra laterale. Non serve Rust, non serve compilare Postamon.

## Come arriva agli utenti

1. Scrivi l'add-on in `addons/<id>/` e lo provi nel tuo Postamon con la **modalità sviluppatore**.
2. Apri una **pull request** su questo repo. Un controllo automatico verifica formato, permessi e regole.
3. Il codice viene **rivisto**. Dopo il merge il manutentore pubblica il catalogo, firmato con la chiave degli
   add-on.
4. Gli utenti lo trovano nel **marketplace** di Postamon (icona del puzzle in basso a sinistra), vedono cosa potrà
   fare e lo installano. Possono disattivarlo o disinstallarlo quando vogliono: disinstallare cancella anche i
   suoi dati.

Postamon scarica solo gli add-on che l'utente installa e solo dal catalogo firmato. Se il pacchetto scaricato non
corrisponde al catalogo (SHA-256) o la firma non torna, non installa niente.

## Per iniziare

```sh
git clone https://github.com/Xp67/Postamon.git
cp -r Postamon/addons/modelli-risposta Postamon/addons/mio-addon
```

1. In `addons/mio-addon/addon.json` cambia almeno `id` (uguale al nome della cartella), `name`, `author` e
   `description`.
2. In Postamon: icona del puzzle → spunta **Modalità sviluppatore** → **Carica da una cartella** → scegli
   `addons/mio-addon`.
3. Modifica i file e premi **Ricarica** (nel pannello dell'add-on o nell'elenco Installati): Postamon rilegge
   `addon.json` e riapre le pagine.

Un add-on in sviluppo gira con le stesse regole di sicurezza di uno installato; l'unica differenza è che viene
dalla tua cartella e non dal catalogo.

`addons/modelli-risposta` è l'esempio da cui partire: usa entrambi i punti di aggancio, i dati salvati, la
lettura del messaggio e la risposta precompilata.

## La cartella dell'add-on

```
addons/mio-addon/
  addon.json      obbligatorio
  index.html      le pagine indicate nei punti di aggancio
  main.js
  stile.css
  icon.svg        facoltativa
  README.md       facoltativo, consigliato
```

- Solo questi tipi di file: `html js mjs css json svg png jpg jpeg gif webp ico woff woff2 ttf otf txt md`.
- Nomi con lettere, cifre, `.`, `_`, `-` (e `/` per le sottocartelle). Niente file o cartelle che iniziano con
  il punto.
- Al massimo 300 file e 5 MB in tutto.
- La cartella `_postamon/` è riservata: lì Postamon serve la sua libreria.
- **Niente librerie di terze parti nel repo**: `node_modules/`, `vendor/` e le librerie compilate sono esclusi
  da git (vedi `.gitignore`), quindi non arriverebbero nella pull request. L'add-on usa il suo codice e la
  libreria di Postamon.
- **Niente passaggi di compilazione**: nel repo va il codice che gira, leggibile. Codice minificato o generato
  (righe di oltre 400 caratteri) non passa il controllo, perché la revisione deve poterlo leggere. TypeScript,
  bundler e framework vanno bene sul tuo PC, ma nella pull request va il JavaScript finale scritto in modo
  leggibile.

## addon.json

```json
{
  "id": "mio-addon",
  "name": "Il mio add-on",
  "version": "1.0.0",
  "description": "Cosa fa, in una frase che capisce chiunque.",
  "author": "Nome Cognome",
  "api": 1,
  "icon": "icon.svg",
  "permissions": ["message.read"],
  "hooks": [{ "type": "message-button", "title": "Fai qualcosa", "page": "index.html" }]
}
```

| Campo | | |
|---|---|---|
| `id` | obbligatorio | 2-40 caratteri fra `a-z`, `0-9` e `-`, uguale al nome della cartella. Non si cambia più. |
| `name` | obbligatorio | Fino a 60 caratteri, mostrato nel marketplace. |
| `version` | obbligatorio | `1.2.3`. Va alzata a ogni modifica: è così che Postamon propone l'aggiornamento. |
| `description` | obbligatorio | Fino a 300 caratteri. |
| `author` | obbligatorio | Fino a 80 caratteri. |
| `api` | obbligatorio | Versione dell'API degli add-on: oggi `1`. |
| `minPostamon` | facoltativo | Versione minima di Postamon (`0.2.0`), se usi qualcosa arrivato dopo. |
| `icon` | facoltativo | File `.svg` o `.png` del pacchetto, mostrato sui pulsanti e nell'elenco. |
| `permissions` | facoltativo | Cosa può fare l'add-on, vedi sotto. Senza, solo i suoi dati e le sue pagine. |
| `hooks` | obbligatorio | Da 1 a 8 punti di aggancio. |

## Punti di aggancio

| `type` | Dove compare | Contesto |
|---|---|---|
| `message-button` | Pulsante nella barra del messaggio aperto, accanto a Inoltra. Apre la pagina in una finestra sopra la posta. | Il messaggio su cui l'utente ha cliccato: `message.read` e `compose.reply` lavorano su quello. |
| `sidebar-view` | Voce nella sezione "Add-on" della barra laterale. Apre la pagina al posto dell'elenco e del messaggio. | Nessun messaggio. `compose.open` scrive dalla casella dello spazio di lavoro aperto. |

`title` è il testo del pulsante o della voce (fino a 40 caratteri), `page` è la pagina `.html` da aprire.

## Permessi

| Permesso | Cosa vede l'utente all'installazione | Cosa permette |
|---|---|---|
| `message.read` | Legge il messaggio aperto: mittente, destinatari, oggetto, testo e nomi degli allegati | `postamon.message.read()` |
| `compose` | Prepara messaggi e risposte, che invii sempre tu | `postamon.compose.open()` e `postamon.compose.reply()` |
| `network:<dominio>` | Si collega a internet, solo a `<dominio>` | `fetch`/`WebSocket`/immagini verso `https://<dominio>` (solo https, dominio esatto, al massimo 5) |

Chiedi solo ciò che serve. Se un aggiornamento aggiunge permessi, Postamon li mostra di nuovo e aspetta che
l'utente li accetti. Un add-on che legge i messaggi **e** si collega a internet viene segnalato all'utente con
un avviso, e in revisione si guarda con più attenzione dove finiscono i dati.

Senza permessi un add-on può sempre: mostrare le sue pagine, salvare i suoi dati (`postamon.storage`), mostrare
un avviso (`postamon.ui.toast`) e chiudere il suo pannello.

## La libreria

Ogni pagina carica la libreria che Postamon serve accanto all'add-on, poi i propri script:

```html
<link rel="stylesheet" href="_postamon/postamon.css" />  <!-- facoltativo: aspetto di Postamon -->
<script src="_postamon/sdk.js"></script>
<script src="main.js"></script>
```

Tutte le funzioni restituiscono una Promise. Se manca il permesso, o qualcosa non va, la Promise viene rifiutata
con un `Error` dal messaggio già leggibile (in italiano). Per l'autocompletamento nell'editor c'è
[`sdk/postamon-addon.d.ts`](sdk/postamon-addon.d.ts).

| Funzione | Permesso | |
|---|---|---|
| `postamon.context()` | | `{ hook, addon: { id, version }, api, message }`: dove è stato aperto e se c'è un messaggio. |
| `postamon.message.read()` | `message.read` | `{ subject, from, to, cc, date, dateText, text, attachments }`. `from` è `{ name, address }`, `text` il testo senza HTML, `attachments` l'elenco `{ index, name, mime, size }` (solo i nomi, non il contenuto). |
| `postamon.compose.open({ to, cc, subject, body })` | `compose` | Apre un messaggio nuovo precompilato; sotto il testo va la firma della casella. Lo invia l'utente. |
| `postamon.compose.reply({ body, all })` | `compose` | Apre la risposta al messaggio aperto (a tutti con `all: true`), con `body` sopra firma e citazione. |
| `postamon.storage.get(chiave)` | | Il valore salvato, o `null`. |
| `postamon.storage.set(chiave, valore)` | | Salva un valore JSON. Fino a 1 MB in tutto per add-on, chiavi fino a 100 caratteri. |
| `postamon.storage.remove(chiave)` / `.keys()` | | |
| `postamon.ui.toast(testo, tipo)` | | Avviso in basso, preceduto dal nome dell'add-on. `tipo`: `"info"`, `"warn"` o `"error"`. |
| `postamon.ui.close()` | | Chiude il pannello (`message-button`) o torna alla posta (`sidebar-view`). |

```js
// Pulsante sul messaggio: conta le parole e risponde.
const msg = await postamon.message.read();
const parole = msg.text.split(/\s+/).filter(Boolean).length;
document.getElementById("esito").textContent = `${parole} parole da ${msg.from.name || msg.from.address}`;

document.getElementById("rispondi").addEventListener("click", async () => {
  await postamon.compose.reply({ body: "Grazie, ricevuto." });
  await postamon.ui.close();
});
```

## Cosa è bloccato, e perché

Le pagine di un add-on girano in un riquadro isolato. Postamon conserva password e posta, quindi un add-on deve
poter fare solo quello che ha dichiarato:

- **Niente accesso a Postamon** se non attraverso la libreria: l'interfaccia, gli altri messaggi, le altre
  caselle, le password e i file del PC non sono raggiungibili.
- **Niente internet** se non verso i domini dei permessi `network:`. Vale per `fetch`, `XMLHttpRequest`,
  `WebSocket`, immagini, font e fogli di stile: tutto ciò che serve va nel pacchetto.
- **Niente script in linea** (`<script>…</script>`, `onclick="…"`): gli script stanno in file `.js`.
- **Niente `eval` e `new Function`**.
- **Niente `localStorage`, cookie, IndexedDB**: il riquadro non ha un'origine propria. Usa `postamon.storage`.
- **Niente finestre**: `alert`, `confirm`, `window.open` e i link che aprono pagine nuove non funzionano. Per
  chiedere conferma usa un pulsante nella pagina.
- **Niente navigazione fuori**: un riquadro che prova ad andare su un sito esterno viene fermato.

Il controllo automatico segnala queste cose prima della pull request: `node scripts/addons.mjs check mio-addon`.

## Proporre l'add-on

1. `node scripts/addons.mjs check mio-addon` deve finire con ✓.
2. Una pull request per add-on. Tocca solo `addons/<id>/`: le modifiche a Postamon stesso vanno in pull request
   separate.
3. Nella descrizione: cosa fa, perché chiede ogni permesso, e per `network:` cosa manda a quel dominio.
4. Il controllo "Add-on" su GitHub deve essere verde.

**Cosa guarda la revisione**: che il codice faccia quello che dice la descrizione; che i permessi servano
davvero; che con `network:` vada fuori solo ciò che serve, senza testo dei messaggi o indirizzi se non è lo scopo
dichiarato; che non ci sia codice offuscato o scaricato da fuori; che il testo per l'utente sia chiaro.

**Aggiornare un add-on già pubblicato**: stessa strada, con `version` alzata. Il marketplace propone
l'aggiornamento a chi lo ha installato; se ci sono permessi nuovi l'utente deve accettarli di nuovo.

## Per chi pubblica il catalogo

Dopo il merge, dal branch `main` aggiornato:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\pubblica-addon.ps1
```

Lo script ricontrolla tutti gli add-on, li impacchetta in `dist-addons\` (zip senza compressione e con date fisse:
lo stesso sorgente dà sempre lo stesso SHA-256, quindi chiunque può verificare che il pacchetto pubblicato venga
dal codice del repo), firma `addons.json` con `~\.tauri\postamon-addons.key` e carica tutto nella release
`addons` del repo. `-SenzaInvio` si ferma prima di caricare.

La chiave privata degli add-on è diversa da quella degli aggiornamenti di Postamon e non va mai nel repo. Se si
perde, bisogna generarne una nuova e pubblicare una versione di Postamon con la nuova chiave pubblica
(`PUBKEY` in [`src-tauri/src/addons.rs`](src-tauri/src/addons.rs)).

## Versioni dell'API

`api: 1` è la versione descritta qui. Quando l'API cambierà in modo non compatibile diventerà `2`: Postamon
continuerà a far girare gli add-on con `api: 1` per un periodo, e un add-on con `api` più nuova di quella
supportata compare nel catalogo come "Richiede l'API N degli add-on: aggiorna Postamon".

// Parti in comune alle due pagine: i modelli stanno nei dati dell'add-on,
// sotto la chiave "modelli", come elenco di { titolo, testo }.

const PREDEFINITI = [
  {
    titolo: "Conferma di ricezione",
    testo: "Buongiorno {nome},\n\nabbiamo ricevuto il suo messaggio e le rispondiamo al più presto.\n\nCordiali saluti",
  },
  {
    titolo: "Preventivo in preparazione",
    testo:
      "Buongiorno {nome},\n\ngrazie per la richiesta. Stiamo preparando il preventivo e glielo mandiamo entro due giorni lavorativi.\n\nCordiali saluti",
  },
];

/** I modelli salvati; la prima volta quelli predefiniti. */
async function caricaModelli() {
  const salvati = await postamon.storage.get("modelli");
  if (Array.isArray(salvati)) return salvati;
  await postamon.storage.set("modelli", PREDEFINITI);
  return PREDEFINITI.map((m) => ({ ...m }));
}

function salvaModelli(modelli) {
  return postamon.storage.set("modelli", modelli);
}

/** Mette nel testo il nome del mittente ({nome}) e l'oggetto ({oggetto}). */
function compila(testo, messaggio) {
  const nome = (messaggio.from.name || "").trim().split(/\s+/)[0] || "";
  return testo
    .replace(/(\s?)\{nome\}/g, (_, spazio) => (nome ? spazio + nome : ""))
    .replaceAll("{oggetto}", messaggio.subject || "");
}

function mostraErrore(e) {
  postamon.ui.toast(e instanceof Error ? e.message : String(e), "error");
}

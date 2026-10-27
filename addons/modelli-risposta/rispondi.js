// Pulsante sul messaggio aperto: si sceglie un modello, si ritocca il testo
// se serve e Postamon apre la risposta con il testo sopra la citazione.

(async () => {
  const elenco = document.getElementById("elenco");
  const testo = document.getElementById("testo");

  const [messaggio, modelli] = await Promise.all([postamon.message.read(), caricaModelli()]);
  const mittente = messaggio.from.name || messaggio.from.address;
  document.getElementById("per").textContent = `Risposta a ${mittente}: «${messaggio.subject || "(senza oggetto)"}»`;
  if (modelli.length === 0) {
    document.getElementById("vuoto").hidden = false;
    return;
  }

  const scegli = (indice) => {
    elenco.querySelectorAll("button").forEach((b, i) => b.classList.toggle("attivo", i === indice));
    testo.value = compila(modelli[indice].testo, messaggio);
  };
  modelli.forEach((modello, indice) => {
    const voce = document.createElement("button");
    voce.className = "voce";
    voce.textContent = modello.titolo;
    voce.addEventListener("click", () => scegli(indice));
    const li = document.createElement("li");
    li.append(voce);
    elenco.append(li);
  });
  scegli(0);
  document.getElementById("scelta").hidden = false;

  const rispondi = async (all) => {
    await postamon.compose.reply({ body: testo.value, all });
    await postamon.ui.close();
  };
  document.getElementById("rispondi").addEventListener("click", () => rispondi(false).catch(mostraErrore));
  document.getElementById("tutti").addEventListener("click", () => rispondi(true).catch(mostraErrore));
})().catch(mostraErrore);

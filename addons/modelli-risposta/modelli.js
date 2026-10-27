// Vista nella barra laterale: elenco dei modelli e modifica.

(async () => {
  const elenco = document.getElementById("elenco");
  const modulo = document.getElementById("modulo");
  const titolo = document.getElementById("titolo");
  const testo = document.getElementById("testo");
  const aiuto = document.getElementById("aiuto");
  const suggerimento = document.getElementById("suggerimento");

  let modelli = await caricaModelli();
  let aperto = -1;

  const mostra = () => {
    elenco.replaceChildren();
    modelli.forEach((modello, indice) => {
      const voce = document.createElement("button");
      voce.className = indice === aperto ? "voce attivo" : "voce";
      voce.textContent = modello.titolo;
      voce.addEventListener("click", () => apri(indice));
      const li = document.createElement("li");
      li.append(voce);
      elenco.append(li);
    });
    modulo.hidden = aperto < 0;
  };

  const apri = (indice) => {
    aperto = indice;
    titolo.value = modelli[indice]?.titolo ?? "";
    testo.value = modelli[indice]?.testo ?? "";
    mostra();
    titolo.focus();
  };

  document.getElementById("nuovo").addEventListener("click", () => {
    modelli.push({ titolo: "Nuovo modello", testo: "Buongiorno {nome},\n\n" });
    apri(modelli.length - 1);
  });

  modulo.addEventListener("submit", async (e) => {
    e.preventDefault();
    modelli[aperto] = { titolo: titolo.value.trim(), testo: testo.value };
    try {
      await salvaModelli(modelli);
      mostra();
      await postamon.ui.toast("Modello salvato");
    } catch (err) {
      mostraErrore(err);
    }
  });

  document.getElementById("elimina").addEventListener("click", async () => {
    modelli.splice(aperto, 1);
    aperto = -1;
    try {
      await salvaModelli(modelli);
      mostra();
    } catch (err) {
      mostraErrore(err);
    }
  });

  aiuto.addEventListener("click", () => {
    suggerimento.hidden = !suggerimento.hidden;
    aiuto.setAttribute("aria-expanded", String(!suggerimento.hidden));
  });

  mostra();
  if (modelli.length > 0) apri(0);
})().catch(mostraErrore);

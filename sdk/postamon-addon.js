// Libreria degli add-on di Postamon (API 1). Postamon la serve a ogni
// add-on all'indirizzo relativo `_postamon/sdk.js`:
//
//   <script src="_postamon/sdk.js"></script>
//   <script src="mio.js"></script>
//
// Espone `window.postamon`. Ogni chiamata restituisce una Promise; se il
// permesso non c'è o qualcosa va storto la Promise viene rifiutata con un
// Error dal messaggio già leggibile. Guida completa: ADDONS.md.
(() => {
  "use strict";

  let nextId = 1;
  const waiting = new Map();

  window.addEventListener("message", (event) => {
    if (event.source !== window.parent) return;
    const reply = event.data;
    if (!reply || reply.postamon !== 1 || !waiting.has(reply.id)) return;
    const { resolve, reject } = waiting.get(reply.id);
    waiting.delete(reply.id);
    if (typeof reply.error === "string") reject(new Error(reply.error));
    else resolve(reply.result);
  });

  const call = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const id = nextId++;
      waiting.set(id, { resolve, reject });
      // L'origine dell'interfaccia di Postamon non è nota all'add-on (e il
      // riquadro ha un'origine opaca): il messaggio va solo al genitore.
      window.parent.postMessage({ postamon: 1, id, method, params }, "*");
    });

  const postamon = Object.freeze({
    api: 1,
    /** Dove è stato aperto l'add-on: { hook, addon: { id, version }, api, message }. */
    context: () => call("context"),
    message: Object.freeze({
      /** Il messaggio aperto (permesso "message.read"). */
      read: () => call("message.read"),
    }),
    compose: Object.freeze({
      /** Nuovo messaggio precompilato (permesso "compose"): { to, cc, subject, body }. */
      open: (draft = {}) => call("compose.open", draft),
      /** Risposta al messaggio aperto (permesso "compose"): { body, all }. */
      reply: (draft = {}) => call("compose.reply", draft),
    }),
    storage: Object.freeze({
      /** Dati dell'add-on, salvati da Postamon (fino a 1 MB, valori JSON). */
      get: (key) => call("storage.get", { key }),
      set: (key, value) => call("storage.set", { key, value }),
      remove: (key) => call("storage.remove", { key }),
      keys: () => call("storage.keys"),
    }),
    ui: Object.freeze({
      /** Avviso in basso nella finestra di Postamon; kind: "info", "warn" o "error". */
      toast: (text, kind = "info") => call("ui.toast", { text, kind }),
      /** Chiude il pannello dell'add-on. */
      close: () => call("ui.close"),
    }),
  });

  Object.defineProperty(window, "postamon", { value: postamon, enumerable: true });
})();

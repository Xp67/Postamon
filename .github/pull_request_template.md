<!-- Per un add-on segui ADDONS.md. Per una modifica a Postamon stesso descrivi cosa cambia e perché. -->

## Cosa fa

<!-- In poche righe, come lo spiegheresti a chi lo installa. -->

## Add-on (se la pull request ne aggiunge o aggiorna uno)

- Cartella: `addons/<id>/`
- Versione:
- Permessi e perché servono:
  - `message.read`:
  - `compose`:
  - `network:<dominio>`: cosa viene mandato a quel dominio, e quando

- [ ] `node scripts/addons.mjs check <id>` finisce con ✓
- [ ] provato nel mio Postamon con la modalità sviluppatore
- [ ] la pull request tocca solo `addons/<id>/`
- [ ] niente codice minificato, generato o scaricato da fuori
- [ ] se è un aggiornamento, `version` è stata alzata

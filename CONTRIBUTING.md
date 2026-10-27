# Contribuire a Postamon

## Add-on

Il modo più semplice per aggiungere una funzione a Postamon è un add-on: una piccola app web in `addons/<id>/`,
proposta con una pull request. La guida completa è [ADDONS.md](ADDONS.md): come partire dall'esempio, provarlo
con la modalità sviluppatore, quali permessi chiedere e cosa guarda la revisione.

In breve:

1. Copia `addons/modelli-risposta` in `addons/<il-tuo-id>/` e modificalo.
2. Provalo in Postamon: icona del puzzle → Modalità sviluppatore → Carica da una cartella.
3. `node scripts/addons.mjs check <il-tuo-id>` deve finire con ✓.
4. Apri la pull request compilando il modello (permessi e perché servono).

## Modifiche a Postamon

Per cambiare l'app stessa (Rust in `src-tauri/`, interfaccia in `src/`, server della chat in `server/`) apri prima
una issue che spieghi il problema o la proposta: è più facile mettersi d'accordo sul come prima di scrivere il
codice. Per provare: vedi "Sviluppo" nel [README](README.md).

Una pull request per argomento; un add-on e una modifica a Postamon vanno in pull request separate.

## Licenza

La licenza del repo non è ancora stata scelta: le pull request esterne si accettano quando sarà aggiunto il file
`LICENSE`.

# Modelli di risposta

Add-on d'esempio di Postamon, da copiare come punto di partenza.

- **Barra laterale → Modelli di risposta**: elenco dei modelli, da scrivere e modificare. `{nome}` diventa il nome
  di chi ha scritto, `{oggetto}` l'oggetto del messaggio.
- **Messaggio aperto → Rispondi con un modello**: si sceglie il modello, si ritocca il testo e Postamon apre la
  risposta con il testo sopra la citazione. Il messaggio lo invii tu.

Permessi: `message.read` (per il nome del mittente e l'oggetto) e `compose` (per aprire la risposta). I modelli
stanno nei dati dell'add-on (`postamon.storage`) e si cancellano se lo disinstalli.

import { useState } from "react";
import { Check, LoaderCircle, X } from "lucide-react";
import { api, errorText, type Account } from "../api";
import { Help } from "./Help";

interface Props {
  account: Account;
  onClose: () => void;
  onSaved: (account: Account) => void;
}

export function SignatureDialog({ account, onClose, onSaved }: Props) {
  const [text, setText] = useState(account.signature);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const save = async () => {
    setBusy(true);
    try {
      onSaved(await api.setSignature(account.id, text));
    } catch (e) {
      setError(errorText(e));
      setBusy(false);
    }
  };

  return (
    <div className="overlay" onKeyDown={(e) => e.key === "Escape" && onClose()}>
      <div className="dialog small">
        <div className="dialog-head">
          <h3>Firma di {account.email}</h3>
          <button className="btn btn-ghost icon-btn" onClick={onClose} aria-label="Chiudi">
            <X />
          </button>
        </div>
        <div className="dialog-body">
          <div className="form-row">
            <label htmlFor="signature">
              Testo della firma
              <Help>
                Si aggiunge in fondo ai nuovi messaggi e sopra la citazione nelle risposte. Alla prima configurazione
                Postamon prova a copiarla da Outlook.
              </Help>
            </label>
            <textarea id="signature" className="input" value={text} onChange={(e) => setText(e.target.value)} autoFocus />
          </div>
          {error && <div className="form-error">{error}</div>}
          <div className="card-actions">
            <button className="btn btn-primary" onClick={() => void save()} disabled={busy}>
              {busy ? <LoaderCircle className="spin" /> : <Check />}
              Salva firma
            </button>
            <button className="btn" onClick={onClose}>
              Annulla
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

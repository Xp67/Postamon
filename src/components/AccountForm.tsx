import { useState, type FormEvent } from "react";
import { ChevronDown, LoaderCircle, Mail, Plus } from "lucide-react";
import { api, errorText, type Account } from "../api";
import { Help } from "./Help";

// Le caselle aziendali stanno su Aruba, con i nomi dei server del gestore.
const DEFAULT_SERVERS = { imapHost: "in.postassl.it", imapPort: 993, smtpHost: "out.postassl.it", smtpPort: 465 };

interface Props {
  first?: boolean;
  onAdded: (account: Account) => void;
  onCancel?: () => void;
}

export function AccountForm({ first, onAdded, onCancel }: Props) {
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [servers, setServers] = useState(DEFAULT_SERVERS);
  const [showServers, setShowServers] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError("");
    try {
      const account = await api.addAccount({ name: name || email.split("@")[0], email, password, ...servers });
      onAdded(account);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form className="card" onSubmit={submit}>
      {first && (
        <div className="card-brand">
          <span className="brand-mark">
            <Mail />
          </span>
          Postamon
        </div>
      )}
      <h1>{first ? "Aggiungi la tua casella" : "Aggiungi una casella"}</h1>

      <div className="form-row">
        <label htmlFor="acc-email">Indirizzo email</label>
        <input
          id="acc-email"
          className="input"
          type="email"
          required
          autoFocus
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          placeholder="nome@dimensionesalotto.it"
        />
      </div>

      <div className="form-row">
        <label htmlFor="acc-password">
          Password
          <Help>
            La stessa che usi in Outlook. Postamon la tiene nella Gestione credenziali di Windows, cifrata, e non la
            scrive in nessun file.
          </Help>
        </label>
        <input
          id="acc-password"
          className="input"
          type="password"
          required
          value={password}
          onChange={(e) => setPassword(e.target.value)}
        />
      </div>

      <div className="form-row">
        <label htmlFor="acc-name">
          Nome mostrato
          <Help>Il nome che vede chi riceve i tuoi messaggi, accanto all'indirizzo.</Help>
        </label>
        <input
          id="acc-name"
          className="input"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="Marco"
        />
      </div>

      <div className="servers-toggle">
        <button
          type="button"
          className={`btn btn-ghost btn-small ${showServers ? "open" : ""}`}
          onClick={() => setShowServers(!showServers)}
          aria-expanded={showServers}
        >
          Server di posta
          <ChevronDown className="chev" />
        </button>
        <Help>
          Per le caselle aziendali sono già giusti: in.postassl.it per ricevere e out.postassl.it per inviare, i server
          Aruba del gestore. Cambiali solo se ti danno indirizzi diversi.
        </Help>
      </div>

      {showServers && (
        <div className="servers">
          <div className="form-row">
            <label>Ricezione (IMAP, SSL)</label>
            <div className="host-port">
              <input
                className="input"
                value={servers.imapHost}
                onChange={(e) => setServers({ ...servers, imapHost: e.target.value })}
              />
              <input
                className="input"
                type="number"
                value={servers.imapPort}
                onChange={(e) => setServers({ ...servers, imapPort: Number(e.target.value) })}
              />
            </div>
          </div>
          <div className="form-row">
            <label>Invio (SMTP)</label>
            <div className="host-port">
              <input
                className="input"
                value={servers.smtpHost}
                onChange={(e) => setServers({ ...servers, smtpHost: e.target.value })}
              />
              <input
                className="input"
                type="number"
                value={servers.smtpPort}
                onChange={(e) => setServers({ ...servers, smtpPort: Number(e.target.value) })}
              />
            </div>
          </div>
        </div>
      )}

      {error && <div className="form-error">{error}</div>}

      <div className="card-actions" style={{ marginTop: error ? 14 : 6 }}>
        <button className="btn btn-primary" type="submit" disabled={busy}>
          {busy ? <LoaderCircle className="spin" /> : <Plus />}
          {busy ? "Verifico l'accesso…" : "Aggiungi casella"}
        </button>
        {onCancel && (
          <button className="btn" type="button" onClick={onCancel} disabled={busy}>
            Annulla
          </button>
        )}
      </div>
    </form>
  );
}

import type { ReactNode } from "react";
import { FolderInput, LoaderCircle, Puzzle, Send, ShieldAlert, Sparkles, SquarePen, Trash, TriangleAlert, X } from "lucide-react";
import { permissionText, type ClaudeAnswer, type ClaudeMessageBrief, type ClaudeRequest } from "../api";
import { listDate } from "../format";

interface Props {
  request: ClaudeRequest;
  /** Richieste in attesa, compresa questa. */
  total: number;
  busy: boolean;
  /** Si può aprire il messaggio per modificarlo (nessun altro in scrittura). */
  canEdit: boolean;
  onAnswer: (answer: ClaudeAnswer) => void;
}

function clientText(client: string): string {
  return client === "pannello" ? "dal pannello Claude" : `da ${client}`;
}

function Messages({ list }: { list: ClaudeMessageBrief[] }) {
  return (
    <ul className="claude-request-list">
      {list.map((m) => (
        <li key={m.uid}>
          <span className="claude-request-subject">{m.subject || "(senza oggetto)"}</span>
          <span className="claude-request-meta">
            {m.from} · {listDate(m.date)}
          </span>
        </li>
      ))}
    </ul>
  );
}

function Field({ label, value }: { label: string; value: string }) {
  return (
    <div className="field">
      <label>{label}</label>
      <div className="field-static selectable" title={value}>
        {value}
      </div>
    </div>
  );
}

/**
 * Un'azione chiesta da Claude, da accettare o rifiutare. Claude la chiede
 * dopo aver letto delle email: qui si vede per intero cosa farà.
 */
export function ClaudeRequestDialog({ request, total, busy, canEdit, onAnswer }: Props) {
  let title: string;
  let content: ReactNode;
  let accept: { label: string; icon: typeof Send; danger?: boolean };
  switch (request.kind) {
    case "send": {
      const d = request.detail.draft;
      title = "Claude vuole inviare questo messaggio";
      content = (
        <>
          <div className="claude-request-fields">
            <Field label="Da" value={request.detail.from} />
            <Field label="A" value={d.to} />
            {d.cc && <Field label="Cc" value={d.cc} />}
            {d.bcc && <Field label="Ccn" value={d.bcc} />}
            <Field label="Oggetto" value={d.subject} />
            <Field label="Firma" value={request.detail.signature ?? "nessuna"} />
          </div>
          <div className="claude-request-body selectable">{d.quote ? `${d.body}\n\n${d.quote}` : d.body}</div>
        </>
      );
      accept = { label: "Invia", icon: Send };
      break;
    }
    case "move":
      title = `Claude vuole spostare ${request.detail.messages.length === 1 ? "un messaggio" : `${request.detail.messages.length} messaggi`}`;
      content = (
        <div className="dialog-body">
          <p className="claude-request-lead">
            Da <b>{request.detail.folder}</b> a <b>{request.detail.destination}</b>, nella casella{" "}
            {request.detail.account}
          </p>
          <Messages list={request.detail.messages} />
        </div>
      );
      accept = { label: "Sposta", icon: FolderInput };
      break;
    case "trash": {
      const n = request.detail.messages.length;
      const what = n === 1 ? "un messaggio" : `${n} messaggi`;
      title = request.detail.permanent ? `Claude vuole eliminare per sempre ${what}` : `Claude vuole cestinare ${what}`;
      content = (
        <div className="dialog-body">
          {request.detail.permanent && (
            <p className="claude-request-warn">
              <TriangleAlert />
              Sono in {request.detail.folder}: eliminati da qui non si recuperano.
            </p>
          )}
          <Messages list={request.detail.messages} />
        </div>
      );
      accept = { label: request.detail.permanent ? "Elimina per sempre" : "Cestina", icon: Trash, danger: true };
      break;
    }
    case "addon-install": {
      const a = request.detail.addon;
      title = a.update ? `Claude vuole aggiornare ${a.name}` : `Claude vuole installare ${a.name}`;
      content = (
        <div className="dialog-body">
          <p className="claude-request-lead">
            <b>
              {a.name} {a.version}
            </b>{" "}
            di {a.author}
            <br />
            {a.description}
          </p>
          {a.permissions.length > 0 && (
            <ul className="consent-list">
              {a.permissions.map((p) => {
                const t = permissionText(p);
                return (
                  <li key={p} className={t.warn ? "warn" : ""}>
                    {t.warn ? <ShieldAlert /> : <Puzzle />}
                    <span>{t.text}</span>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      );
      accept = { label: a.update ? "Aggiorna" : "Installa", icon: Puzzle };
      break;
    }
    case "addon-uninstall":
      title = `Claude vuole togliere ${request.detail.addon.name}`;
      content = (
        <div className="dialog-body">
          <p className="claude-request-lead">
            {request.detail.addon.developerMode
              ? "Si cancellano i suoi dati; la cartella del sorgente resta com'è."
              : "Si cancellano anche i suoi dati."}
          </p>
        </div>
      );
      accept = { label: "Togli", icon: Trash, danger: true };
      break;
  }
  const Icon = accept.icon;

  return (
    <div className="overlay claude-overlay">
      <div className="dialog small claude-request" role="alertdialog" aria-label={title} data-request={request.id}>
        <div className="dialog-head">
          <Sparkles className="claude-mark" />
          <h3>{title}</h3>
          <span className="tag">{clientText(request.client)}</span>
        </div>
        {content}
        <div className="addons-foot">
          {total > 1 && <span className="claude-request-count">altre {total - 1} in attesa</span>}
          <span className="spacer" />
          <button className="btn" onClick={() => onAnswer("reject")} disabled={busy}>
            <X />
            Rifiuta
          </button>
          {request.kind === "send" && (
            <button
              className="btn"
              onClick={() => onAnswer("edit")}
              disabled={busy || !canEdit}
              title={canEdit ? "Apri nella finestra di scrittura" : "Chiudi prima il messaggio che stai scrivendo"}
            >
              <SquarePen />
              Modifica
            </button>
          )}
          <button
            className={`btn ${accept.danger ? "btn-danger" : "btn-primary"}`}
            onClick={() => onAnswer("accept")}
            disabled={busy}
          >
            {busy ? <LoaderCircle className="spin" /> : <Icon />}
            {accept.label}
          </button>
        </div>
      </div>
    </div>
  );
}

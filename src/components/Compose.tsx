import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { File, LoaderCircle, Paperclip, Send, TriangleAlert, X } from "lucide-react";
import { api, errorText, type Account, type Attachment, type FileInfo, type MsgRef, type SendReport } from "../api";
import { fileSize } from "../format";

export interface ComposeInit {
  accountId: string;
  to: string;
  cc: string;
  bcc: string;
  subject: string;
  body: string;
  inReplyTo: string | null;
  references: string | null;
  reply: MsgRef | null;
  forward: MsgRef | null;
  forwardAttachments: Attachment[];
}

interface Props {
  accounts: Account[];
  initial: ComposeInit;
  onClose: () => void;
  onSent: (report: SendReport) => void;
}

/** Oltre questa soglia molti server dei destinatari rifiutano il messaggio. */
const RECIPIENT_LIMIT = 20 * 1024 * 1024;

export function Compose({ accounts, initial, onClose, onSent }: Props) {
  const [accountId, setAccountId] = useState(initial.accountId);
  const [to, setTo] = useState(initial.to);
  const [cc, setCc] = useState(initial.cc);
  const [bcc, setBcc] = useState(initial.bcc);
  const [showCopies, setShowCopies] = useState(initial.cc !== "" || initial.bcc !== "");
  const [subject, setSubject] = useState(initial.subject);
  const [body, setBody] = useState(initial.body);
  const [files, setFiles] = useState<FileInfo[]>([]);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState("");
  const [askSubject, setAskSubject] = useState(false);
  const [askDiscard, setAskDiscard] = useState(false);
  const [dragging, setDragging] = useState(false);
  const bodyRef = useRef<HTMLTextAreaElement>(null);

  // In risposta il cursore va in cima al testo, sopra la citazione.
  useEffect(() => {
    if (initial.to && bodyRef.current) {
      bodyRef.current.focus();
      bodyRef.current.setSelectionRange(0, 0);
      bodyRef.current.scrollTop = 0;
    }
  }, [initial.to]);

  const addPaths = async (paths: string[]) => {
    if (!paths.length) return;
    try {
      const info = await api.filesInfo(paths);
      setFiles((prev) => [...prev, ...info.filter((f) => !prev.some((p) => p.path === f.path))]);
    } catch (e) {
      setError(errorText(e));
    }
  };

  // File trascinati sulla finestra mentre si scrive.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        const p = event.payload;
        if (p.type === "enter" || p.type === "over") setDragging(true);
        else if (p.type === "leave") setDragging(false);
        else if (p.type === "drop") {
          setDragging(false);
          void addPaths(p.paths);
        }
      })
      .then((u) => (unlisten = u));
    return () => unlisten?.();
  }, []);

  const attach = async () => {
    const picked = await openDialog({ multiple: true, title: "Allega file" });
    if (picked) await addPaths(Array.isArray(picked) ? picked : [picked]);
  };

  const totalSize = useMemo(
    () => files.reduce((s, f) => s + f.size, 0) + initial.forwardAttachments.reduce((s, a) => s + a.size, 0),
    [files, initial.forwardAttachments],
  );

  const dirty = to !== initial.to || cc !== initial.cc || subject !== initial.subject || body !== initial.body || files.length > 0;

  const close = () => {
    if (dirty && !askDiscard) setAskDiscard(true);
    else onClose();
  };

  const send = async () => {
    if (sending) return;
    if (!subject.trim() && !askSubject) {
      setAskSubject(true);
      return;
    }
    setSending(true);
    setError("");
    try {
      const report = await api.send({
        accountId,
        to,
        cc,
        bcc,
        subject,
        body,
        attachments: files.map((f) => f.path),
        inReplyTo: initial.inReplyTo,
        references: initial.references,
        reply: initial.reply,
        forward: initial.forward,
      });
      onSent(report);
    } catch (e) {
      setError(errorText(e));
      setSending(false);
    }
  };

  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Enter" && e.ctrlKey) {
      e.preventDefault();
      void send();
    } else if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
  };

  const title = initial.reply ? "Risposta" : initial.forward ? "Inoltro" : "Nuovo messaggio";

  return (
    <div className="overlay">
      <div className="dialog compose" onKeyDown={onKey} style={{ position: "relative" }}>
        <div className="dialog-head">
          <h3>{title}</h3>
          <button className="btn btn-ghost icon-btn" onClick={close} aria-label="Chiudi">
            <X />
          </button>
        </div>

        <div className="field">
          <label>Da</label>
          {accounts.length > 1 ? (
            <select value={accountId} onChange={(e) => setAccountId(e.target.value)}>
              {accounts.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name ? `${a.name} <${a.email}>` : a.email}
                </option>
              ))}
            </select>
          ) : (
            <span className="field-static">
              {accounts[0]?.name ? `${accounts[0].name} <${accounts[0].email}>` : accounts[0]?.email}
            </span>
          )}
          <span />
        </div>
        <AddressField label="A" value={to} onChange={setTo} autoFocus={!initial.to}>
          {!showCopies && (
            <button className="btn btn-ghost btn-small" onClick={() => setShowCopies(true)}>
              Cc e Ccn
            </button>
          )}
        </AddressField>
        {showCopies && <AddressField label="Cc" value={cc} onChange={setCc} />}
        {showCopies && <AddressField label="Ccn" value={bcc} onChange={setBcc} />}
        <div className="field">
          <label htmlFor="compose-subject">Oggetto</label>
          <input
            id="compose-subject"
            value={subject}
            onChange={(e) => {
              setSubject(e.target.value);
              setAskSubject(false);
            }}
          />
          <span />
        </div>

        <textarea
          ref={bodyRef}
          className="compose-body"
          value={body}
          onChange={(e) => setBody(e.target.value)}
          spellCheck
          lang="it"
        />

        {(files.length > 0 || initial.forwardAttachments.length > 0) && (
          <div className="compose-files">
            {initial.forwardAttachments.map((a) => (
              <div className="chip" key={`fw-${a.index}`} title="Allegato del messaggio inoltrato">
                <span className="chip-open">
                  <File className="kind" />
                  <span className="chip-text">
                    <span className="chip-name">{a.name}</span>
                    <span className="chip-size">{fileSize(a.size)}</span>
                  </span>
                </span>
              </div>
            ))}
            {files.map((f) => (
              <div className="chip" key={f.path} title={f.path}>
                <span className="chip-open">
                  <File className="kind" />
                  <span className="chip-text">
                    <span className="chip-name">{f.name}</span>
                    <span className="chip-size">{fileSize(f.size)}</span>
                  </span>
                </span>
                <button
                  className="btn btn-ghost icon-btn"
                  aria-label={`Togli ${f.name}`}
                  onClick={() => setFiles(files.filter((x) => x.path !== f.path))}
                >
                  <X />
                </button>
              </div>
            ))}
          </div>
        )}

        {totalSize > RECIPIENT_LIMIT && (
          <div className="compose-warning">
            <TriangleAlert />
            Allegati per {fileSize(totalSize)}: molti server rifiutano i messaggi oltre 20-25 MB.
          </div>
        )}
        {askSubject && (
          <div className="compose-warning">
            <TriangleAlert />
            Manca l'oggetto. Premi di nuovo Invia per mandarlo così.
          </div>
        )}
        {askDiscard && (
          <div className="compose-warning">
            <TriangleAlert />
            <span className="grow" style={{ flex: 1 }}>
              Il messaggio non è stato inviato. Vuoi scartarlo?
            </span>
            <button className="btn btn-small btn-danger" onClick={onClose}>
              Scarta
            </button>
            <button className="btn btn-small" onClick={() => setAskDiscard(false)}>
              Continua a scrivere
            </button>
          </div>
        )}
        {error && (
          <div style={{ padding: "10px 18px 0" }}>
            <div className="form-error">{error}</div>
          </div>
        )}

        <div className="compose-foot">
          <button className="btn btn-primary" onClick={() => void send()} disabled={sending}>
            {sending ? <LoaderCircle className="spin" /> : <Send />}
            {sending ? "Invio in corso…" : "Invia"}
          </button>
          <button className="btn" onClick={() => void attach()} disabled={sending}>
            <Paperclip />
            Allega
          </button>
          <span className="grow" />
        </div>

        {dragging && <div className="drop-hint">Rilascia qui i file da allegare</div>}
      </div>
    </div>
  );
}

/** Campo indirizzi con i suggerimenti presi dalla posta già ricevuta. */
function AddressField({
  label,
  value,
  onChange,
  autoFocus,
  children,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  autoFocus?: boolean;
  children?: React.ReactNode;
}) {
  const [suggestions, setSuggestions] = useState<string[]>([]);
  const [active, setActive] = useState(0);
  const [focused, setFocused] = useState(false);
  const lastToken = value.split(/[,;]/).pop()?.trim() ?? "";

  useEffect(() => {
    if (lastToken.length < 2) {
      setSuggestions([]);
      return;
    }
    let alive = true;
    const t = window.setTimeout(() => {
      api
        .contacts(lastToken)
        .then((list) => {
          if (alive) {
            setSuggestions(list.filter((s) => !value.includes(s)));
            setActive(0);
          }
        })
        .catch(() => {});
    }, 120);
    return () => {
      alive = false;
      window.clearTimeout(t);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [lastToken]);

  const pick = (s: string) => {
    const cut = Math.max(value.lastIndexOf(","), value.lastIndexOf(";"));
    const head = cut >= 0 ? value.slice(0, cut + 1) + " " : "";
    onChange(`${head}${s}, `);
    setSuggestions([]);
  };

  const onKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (!suggestions.length) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((a) => Math.min(suggestions.length - 1, a + 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((a) => Math.max(0, a - 1));
    } else if (e.key === "Enter" || e.key === "Tab") {
      e.preventDefault();
      pick(suggestions[active]);
    } else if (e.key === "Escape") {
      e.stopPropagation();
      setSuggestions([]);
    }
  };

  return (
    <div className="field">
      <label>{label}</label>
      <input
        value={value}
        autoFocus={autoFocus}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={onKey}
        onFocus={() => setFocused(true)}
        onBlur={() => window.setTimeout(() => setFocused(false), 150)}
        spellCheck={false}
      />
      {children ?? <span />}
      {focused && suggestions.length > 0 && (
        <div className="suggestions">
          {suggestions.map((s, i) => (
            <button key={s} className={i === active ? "active" : ""} onMouseDown={(e) => e.preventDefault()} onClick={() => pick(s)}>
              {s}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

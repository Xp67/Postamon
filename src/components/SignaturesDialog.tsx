import { useEffect, useState } from "react";
import { Check, Import, LoaderCircle, Plus, Signature as SignatureIcon, Trash, TriangleAlert, X } from "lucide-react";
import { api, errorText, type Account, type Signature } from "../api";
import { Help } from "./Help";
import { SignatureEditor } from "./SignatureEditor";

interface Props {
  accounts: Account[];
  /** Casella da cui si è aperto: la sua firma è quella che si apre per prima. */
  accountId: string;
  onClose: () => void;
  onAccounts: (accounts: Account[]) => void;
}

/** Firma aperta nell'editor; `id` nullo finché una firma nuova non è salvata. */
interface Draft {
  id: string | null;
  name: string;
  html: string;
  key: string;
}

/** Elenco delle firme, firma di ogni casella ed editor. */
export function SignaturesDialog({ accounts, accountId, onClose, onAccounts }: Props) {
  const [list, setList] = useState<Signature[] | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [dirty, setDirty] = useState(false);
  // Cosa fare dopo aver salvato o scartato le modifiche (aprire un'altra firma, chiudere).
  const [pending, setPending] = useState<(() => void) | null>(null);
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState("");
  const [askDelete, setAskDelete] = useState(false);
  const [outlook, setOutlook] = useState<string[] | null>(null);
  const [chosen, setChosen] = useState<string[]>([]);

  const open = (s: Signature | null) => {
    setDraft(
      s
        ? { id: s.id, name: s.name, html: s.html, key: s.id }
        : { id: null, name: "Nuova firma", html: "", key: `nuova-${Date.now()}` },
    );
    setDirty(false);
    setAskDelete(false);
    setSaved(false);
    setError("");
    setOutlook(null);
  };

  useEffect(() => {
    api
      .signatures()
      .then((all) => {
        setList(all);
        const acc = accounts.find((a) => a.id === accountId);
        open(all.find((s) => s.id === acc?.signatureNew) ?? all[0] ?? null);
      })
      .catch((e) => setError(errorText(e)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const guarded = (action: () => void) => {
    if (dirty) setPending(() => action);
    else action();
  };

  const save = async (): Promise<boolean> => {
    if (!draft) return false;
    setBusy(true);
    setError("");
    try {
      const s = await api.saveSignature(draft.id, draft.name, draft.html);
      setList((l) => (l?.some((x) => x.id === s.id) ? l.map((x) => (x.id === s.id ? s : x)) : [...(l ?? []), s]));
      // Stessa chiave: l'editor resta com'è, senza ricaricare il testo.
      setDraft((d) => d && { ...d, id: s.id, name: s.name, html: s.html });
      setDirty(false);
      setSaved(true);
      window.setTimeout(() => setSaved(false), 2000);
      // La prima firma di una casella che non ne ha: la usa subito.
      const acc = accounts.find((a) => a.id === accountId);
      if (draft.id === null && acc && !acc.signatureNew && !acc.signatureReply) {
        const updated = await api.setSignatures(acc.id, s.id, s.id);
        onAccounts(accounts.map((a) => (a.id === updated.id ? updated : a)));
      }
      return true;
    } catch (e) {
      setError(errorText(e));
      return false;
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    setAskDelete(false);
    if (!draft?.id) {
      open(list?.[0] ?? null);
      return;
    }
    try {
      onAccounts(await api.deleteSignature(draft.id));
      const rest = (list ?? []).filter((s) => s.id !== draft.id);
      setList(rest);
      open(rest[0] ?? null);
    } catch (e) {
      setError(errorText(e));
    }
  };

  const setDefault = async (acc: Account, which: "new" | "reply", id: string) => {
    const value = id || null;
    try {
      const updated = await api.setSignatures(
        acc.id,
        which === "new" ? value : acc.signatureNew,
        which === "reply" ? value : acc.signatureReply,
      );
      onAccounts(accounts.map((a) => (a.id === updated.id ? updated : a)));
    } catch (e) {
      setError(errorText(e));
    }
  };

  const showOutlook = async () => {
    try {
      const names = await api.outlookSignatures();
      setOutlook(names);
      setChosen(names);
    } catch (e) {
      setError(errorText(e));
    }
  };

  const importOutlook = async () => {
    try {
      const added = await api.importOutlookSignatures(chosen);
      setList((l) => [...(l ?? []), ...added]);
      setOutlook(null);
      if (added[0]) open(added[0]);
    } catch (e) {
      setError(errorText(e));
    }
  };

  // Chi usa una firma, scritto sotto il suo nome nell'elenco.
  const usage = (s: Signature) =>
    accounts
      .filter((a) => a.signatureNew === s.id || a.signatureReply === s.id)
      .map((a) => {
        const both = a.signatureNew === s.id && a.signatureReply === s.id;
        const local = a.email.split("@")[0];
        return both ? `${local}@` : `${local}@ · ${a.signatureNew === s.id ? "nuovi" : "risposte"}`;
      })
      .join(", ");

  // La casella da cui si è aperto per prima.
  const rows = [...accounts].sort((a, b) => Number(b.id === accountId) - Number(a.id === accountId));

  return (
    <div
      className="overlay"
      onKeyDown={(e) => {
        if (e.key === "Escape") guarded(onClose);
      }}
    >
      <div className="dialog signatures-dialog">
        <div className="dialog-head">
          <SignatureIcon />
          <h3>Firme</h3>
          <button className="btn btn-ghost icon-btn" onClick={() => guarded(onClose)} aria-label="Chiudi">
            <X />
          </button>
        </div>

        {list && list.length > 0 && (
          <div className="sig-defaults">
            <div className="sig-defaults-head">
              <span>Firma di ogni casella</span>
              <Help align="left">
                La firma va sotto il testo; nelle risposte e negli inoltri sta fra il testo e il messaggio originale.
                Mentre scrivi puoi cambiarla o toglierla per quel messaggio.
              </Help>
            </div>
            {rows.map((acc) => (
              <div className="sig-default-row" key={acc.id}>
                <span className="sig-default-email">{acc.email}</span>
                <label>
                  Messaggi nuovi
                  <select value={acc.signatureNew ?? ""} onChange={(e) => void setDefault(acc, "new", e.target.value)}>
                    <option value="">Nessuna firma</option>
                    {list.map((s) => (
                      <option key={s.id} value={s.id}>
                        {s.name}
                      </option>
                    ))}
                  </select>
                </label>
                <label>
                  Risposte e inoltri
                  <select
                    value={acc.signatureReply ?? ""}
                    onChange={(e) => void setDefault(acc, "reply", e.target.value)}
                  >
                    <option value="">Nessuna firma</option>
                    {list.map((s) => (
                      <option key={s.id} value={s.id}>
                        {s.name}
                      </option>
                    ))}
                  </select>
                </label>
              </div>
            ))}
          </div>
        )}

        {pending && (
          <div className="compose-warning">
            <TriangleAlert />
            <span style={{ flex: 1 }}>La firma ha modifiche non salvate.</span>
            <button
              className="btn btn-small btn-primary"
              onClick={() =>
                void save().then((ok) => {
                  if (ok) pending();
                  setPending(null);
                })
              }
            >
              <Check />
              Salva
            </button>
            <button
              className="btn btn-small btn-danger"
              onClick={() => {
                setDirty(false);
                pending();
                setPending(null);
              }}
            >
              <Trash />
              Scarta
            </button>
            <button className="btn btn-small" onClick={() => setPending(null)}>
              Continua a modificare
            </button>
          </div>
        )}

        <div className="sig-body">
          <div className="sig-list">
            {list === null && !error && (
              <div className="sig-list-loading">
                <LoaderCircle className="spin" />
              </div>
            )}
            {list?.map((s) => (
              <button
                key={s.id}
                className={`sig-item ${draft?.id === s.id && outlook === null ? "active" : ""}`}
                onClick={() => guarded(() => open(s))}
              >
                <span className="sig-item-name">{s.name}</span>
                {usage(s) && <span className="sig-item-used">{usage(s)}</span>}
              </button>
            ))}
            {draft && draft.id === null && outlook === null && (
              <div className="sig-item active">
                <span className="sig-item-name">{draft.name || "Nuova firma"}</span>
                <span className="sig-item-used">non ancora salvata</span>
              </div>
            )}
            <div className="sig-list-actions">
              <button className="btn btn-small" onClick={() => guarded(() => open(null))}>
                <Plus />
                Nuova firma
              </button>
              <button className="btn btn-small" onClick={() => guarded(() => void showOutlook())}>
                <Import />
                Importa da Outlook
              </button>
            </div>
          </div>

          <div className="sig-edit">
            {outlook !== null ? (
              <div className="sig-import">
                <h4>Firme di Outlook su questo PC</h4>
                {outlook.length === 0 ? (
                  <p>Sono già tutte nell'elenco.</p>
                ) : (
                  outlook.map((name) => (
                    <label key={name} className="sig-import-row">
                      <input
                        type="checkbox"
                        checked={chosen.includes(name)}
                        onChange={(e) =>
                          setChosen((c) => (e.target.checked ? [...c, name] : c.filter((n) => n !== name)))
                        }
                      />
                      {name}
                    </label>
                  ))
                )}
                <div className="card-actions">
                  {outlook.length > 0 && (
                    <button className="btn btn-primary" disabled={chosen.length === 0} onClick={() => void importOutlook()}>
                      <Import />
                      Importa {chosen.length === 1 ? "la firma" : `${chosen.length} firme`}
                    </button>
                  )}
                  <button className="btn" onClick={() => setOutlook(null)}>
                    <X />
                    Chiudi
                  </button>
                </div>
              </div>
            ) : (
              draft && (
                <>
                  <div className="sig-name-row">
                    <label htmlFor="sig-name">Nome</label>
                    <input
                      id="sig-name"
                      className="input"
                      value={draft.name}
                      onChange={(e) => {
                        setDraft({ ...draft, name: e.target.value });
                        setDirty(true);
                      }}
                    />
                  </div>
                  <SignatureEditor
                    html={draft.html}
                    docKey={draft.key}
                    onChange={(html) => {
                      setDraft((d) => d && { ...d, html });
                      setDirty(true);
                    }}
                    onError={setError}
                  />
                  {error && <div className="form-error">{error}</div>}
                  {askDelete && (
                    <div className="compose-warning">
                      <TriangleAlert />
                      <span style={{ flex: 1 }}>
                        Eliminare «{draft.name}»? Le caselle che la usano resteranno senza firma.
                      </span>
                      <button className="btn btn-small btn-danger" onClick={() => void remove()}>
                        <Trash />
                        Elimina
                      </button>
                      <button className="btn btn-small" onClick={() => setAskDelete(false)}>
                        Annulla
                      </button>
                    </div>
                  )}
                  <div className="card-actions sig-actions">
                    <button className="btn btn-primary" onClick={() => void save()} disabled={busy}>
                      {busy ? <LoaderCircle className="spin" /> : <Check />}
                      {saved ? "Firma salvata" : "Salva firma"}
                    </button>
                    <button className="btn" onClick={() => setAskDelete(true)} disabled={busy}>
                      <Trash />
                      Elimina
                    </button>
                  </div>
                </>
              )
            )}
            {!draft && outlook === null && error && <div className="form-error">{error}</div>}
          </div>
        </div>
      </div>
    </div>
  );
}

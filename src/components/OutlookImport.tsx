import { useEffect, useMemo, useRef, useState } from "react";
import { Check, Import, LoaderCircle, RotateCw, TriangleAlert } from "lucide-react";
import { api, errorText, type Contact, type ContactInput, type OutlookFolder, type OutlookPerson } from "../api";
import { Help } from "./Help";
import { MorphWindow } from "./MorphWindow";

interface Props {
  anchor: HTMLElement;
  /** Indirizzo della casella: le cartelle di Outlook della stessa casella partono già scelte. */
  email: string;
  contacts: Contact[];
  onImport: (list: ContactInput[]) => Promise<void>;
  onClose: () => void;
}

type State =
  | { phase: "loading" }
  | { phase: "error"; message: string }
  | { phase: "ready"; folders: OutlookFolder[] };

/** Cosa cambia importando la persona: nuova, completa campi vuoti, niente. */
type Effect = "new" | "fills" | "same";

const FIELDS = ["name", "company", "phone", "notes"] as const;

function effectOf(p: OutlookPerson, existing: Contact | undefined): Effect {
  if (!existing) return "new";
  return FIELDS.some((k) => !existing[k] && p[k]) ? "fills" : "same";
}

/** Cartelle da proporre: quelle della stessa casella, altrimenti tutte; mai i "contatti suggeriti". */
function defaultFolders(folders: OutlookFolder[], email: string): OutlookFolder[] {
  const usable = folders.filter((f) => !/suggerit|suggested/i.test(f.name));
  const own = usable.filter((f) => f.store.toLowerCase() === email.toLowerCase());
  return own.length ? own : usable;
}

function FolderCheck({ state, onChange }: { state: "all" | "some" | "none"; onChange: () => void }) {
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (ref.current) ref.current.indeterminate = state === "some";
  }, [state]);
  return <input ref={ref} type="checkbox" checked={state === "all"} onChange={onChange} />;
}

/** Finestrella che legge la rubrica di Outlook e fa scegliere chi importare. */
export function OutlookImport({ anchor, email, contacts, onImport, onClose }: Props) {
  const [state, setState] = useState<State>({ phase: "loading" });
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  const existing = useMemo(() => new Map(contacts.map((c) => [c.address.toLowerCase(), c])), [contacts]);
  const effect = (p: OutlookPerson) => effectOf(p, existing.get(p.address));

  const load = async () => {
    setState({ phase: "loading" });
    try {
      const { folders } = await api.outlookContacts();
      const start = defaultFolders(folders, email)
        .flatMap((f) => f.people)
        .filter((p) => effectOf(p, existing.get(p.address)) !== "same");
      setChosen(new Set(start.map((p) => p.address)));
      setState({ phase: "ready", folders });
    } catch (e) {
      setState({ phase: "error", message: errorText(e) });
    }
  };

  useEffect(() => {
    void load();
    // Una lettura sola all'apertura; "Riprova" la ripete.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const toggle = (addresses: string[], on: boolean) =>
    setChosen((prev) => {
      const next = new Set(prev);
      for (const a of addresses) {
        if (on) next.add(a);
        else next.delete(a);
      }
      return next;
    });

  const folders = state.phase === "ready" ? state.folders : [];
  // Lo stesso indirizzo può stare in due cartelle: si importa una volta sola, con i dati della prima.
  const picked = useMemo(() => {
    const byAddress = new Map<string, OutlookPerson>();
    for (const p of folders.flatMap((f) => f.people)) {
      if (chosen.has(p.address) && !byAddress.has(p.address)) byAddress.set(p.address, p);
    }
    return [...byAddress.values()];
  }, [folders, chosen]);

  const submit = async (close: () => void) => {
    if (saving || !picked.length) return;
    setSaving(true);
    setError("");
    try {
      await onImport(picked.map(({ name, address, company, phone, notes }) => ({ name, address, company, phone, notes })));
      close();
    } catch (e) {
      setError(errorText(e));
      setSaving(false);
    }
  };

  return (
    <MorphWindow anchor={anchor} label="Importa da Outlook" className="outlook-import" onClose={onClose}>
      {(close) => (
        <div className="outlook-import-body">
          <div className="contact-card-title">
            <Import />
            Importa da Outlook
            <Help>
              Postamon legge i contatti dalla rubrica di Outlook su questo PC, senza cambiarla. Ogni indirizzo email
              diventa un contatto. Di chi è già in rubrica completa solo i campi vuoti: quello che hai scritto resta
              com'è.
            </Help>
          </div>

          {state.phase === "loading" && (
            <div className="outlook-import-state">
              <LoaderCircle className="spin" />
              <div>Leggo la rubrica di Outlook…</div>
            </div>
          )}

          {state.phase === "error" && (
            <div className="outlook-import-state">
              <TriangleAlert className="warn" />
              <div>{state.message}</div>
              <button className="btn btn-small" onClick={() => void load()}>
                <RotateCw />
                Riprova
              </button>
            </div>
          )}

          {state.phase === "ready" && folders.length === 0 && (
            <div className="outlook-import-state">
              <div>In Outlook non ci sono contatti con un indirizzo email.</div>
            </div>
          )}

          {state.phase === "ready" && folders.length > 0 && (
            <div className="outlook-import-list">
              {folders.map((f, i) => {
                const open = f.people.filter((p) => effect(p) !== "same").map((p) => p.address);
                const on = open.filter((a) => chosen.has(a)).length;
                const check = on === 0 ? "none" : on === open.length ? "all" : "some";
                return (
                  <div key={`${f.store}/${f.name}/${i}`} className="outlook-folder">
                    <label className="outlook-folder-head">
                      {open.length > 0 ? (
                        <FolderCheck state={check} onChange={() => toggle(open, check !== "all")} />
                      ) : (
                        <input type="checkbox" disabled />
                      )}
                      <span className="outlook-folder-name">{f.name}</span>
                      <span className="outlook-folder-meta">
                        {f.store}
                        {f.withoutEmail > 0 && ` · ${f.withoutEmail} senza email, non importabili`}
                      </span>
                    </label>
                    {f.people.map((p) => {
                      const e = effect(p);
                      return (
                        <label key={p.address} className={`outlook-person ${e === "same" ? "same" : ""}`}>
                          <input
                            type="checkbox"
                            checked={e !== "same" && chosen.has(p.address)}
                            disabled={e === "same"}
                            onChange={(ev) => toggle([p.address], ev.target.checked)}
                          />
                          <span className="outlook-person-text">
                            <span className="outlook-person-name">{p.name || p.address}</span>
                            <span className="outlook-person-meta">
                              {[p.name ? p.address : "", p.company, p.group ? `gruppo «${p.group}»` : ""]
                                .filter(Boolean)
                                .join(" · ")}
                            </span>
                          </span>
                          {e === "fills" && <span className="outlook-tag">Completa</span>}
                          {e === "same" && <span className="outlook-tag">Già in rubrica</span>}
                        </label>
                      );
                    })}
                  </div>
                );
              })}
            </div>
          )}

          {error && <div className="contact-card-error">{error}</div>}
          <div className="contact-card-actions">
            <button type="button" className="btn btn-small" onClick={close}>
              Annulla
            </button>
            <button
              className="btn btn-primary btn-small"
              disabled={state.phase !== "ready" || !picked.length || saving}
              onClick={() => void submit(close)}
            >
              {saving ? <LoaderCircle className="spin" /> : <Check />}
              {picked.length === 0
                ? "Importa"
                : picked.length === 1
                  ? "Importa 1 contatto"
                  : `Importa ${picked.length} contatti`}
            </button>
          </div>
        </div>
      )}
    </MorphWindow>
  );
}

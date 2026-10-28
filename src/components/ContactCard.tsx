import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import { Check, LoaderCircle, UserPen, UserPlus } from "lucide-react";
import { errorText, type ContactInput } from "../api";
import { MorphWindow } from "./MorphWindow";

interface Props {
  /** Pulsante da cui la finestra nasce e in cui torna chiudendosi. */
  anchor: HTMLElement;
  title: string;
  initial: ContactInput;
  /** Aggiungendo il mittente di una mail l'indirizzo è il suo e non si cambia. */
  addressLocked?: boolean;
  /** Se rifiuta, l'errore resta nella finestra. */
  onSave: (contact: ContactInput) => Promise<unknown>;
  onClose: () => void;
  /** Azienda dei colleghi con lo stesso dominio aziendale, se c'è. */
  suggestCompany: (address: string) => Promise<string | null>;
  /** Aziende già in rubrica, proposte mentre si scrive. */
  companyNames: string[];
}

/** Finestrella con i dati di una persona, che si apre dal pulsante che la chiama. */
export function ContactCard(props: Props) {
  const { initial, addressLocked, suggestCompany, companyNames } = props;
  const [draft, setDraft] = useState(initial);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const listId = useId();
  // L'azienda segue quella dei colleghi dello stesso dominio finché non la si
  // scrive a mano (o finché il contatto non ne ha già una sua).
  const companyTouched = useRef(initial.company !== "");
  const suggest = useRef(suggestCompany);
  suggest.current = suggestCompany;

  useEffect(() => {
    if (companyTouched.current) return;
    const address = draft.address.trim();
    // Mentre si scrive l'indirizzo si aspetta una pausa prima di chiedere.
    const timer = window.setTimeout(
      () => {
        suggest
          .current(address)
          .then((found) => {
            if (companyTouched.current) return;
            // Se intanto l'indirizzo è cambiato, la risposta è vecchia.
            setDraft((d) => (d.address.trim() === address ? { ...d, company: found ?? "" } : d));
          })
          .catch(() => {
            // senza proposta il campo resta com'è
          });
      },
      address === initial.address ? 0 : 250,
    );
    return () => window.clearTimeout(timer);
  }, [draft.address, initial.address]);

  const set = (key: keyof ContactInput) => (e: { target: { value: string } }) =>
    setDraft((d) => ({ ...d, [key]: e.target.value }));

  const submit = async (e: FormEvent, close: () => void) => {
    e.preventDefault();
    if (saving) return;
    setSaving(true);
    setError("");
    try {
      await props.onSave(draft);
      close();
    } catch (err) {
      setError(errorText(err));
      setSaving(false);
    }
  };

  const Icon = initial.id ? UserPen : UserPlus;

  return (
    <MorphWindow anchor={props.anchor} label={props.title} onClose={props.onClose}>
      {(close) => (
        <form onSubmit={(e) => void submit(e, close)}>
          <div className="contact-card-title">
            <Icon />
            {props.title}
          </div>
          <label className="contact-field">
            <span>Nome</span>
            <input autoFocus value={draft.name} onChange={set("name")} placeholder={draft.address} />
          </label>
          <label className="contact-field">
            <span>Email</span>
            {addressLocked ? (
              <span className="contact-field-static selectable">{draft.address}</span>
            ) : (
              <input type="email" value={draft.address} onChange={set("address")} required />
            )}
          </label>
          <label className="contact-field">
            <span>Azienda</span>
            <input
              value={draft.company}
              list={listId}
              onChange={(e) => {
                companyTouched.current = true;
                set("company")(e);
              }}
              placeholder="Facoltativa"
            />
            <datalist id={listId}>
              {companyNames.map((n) => (
                <option key={n} value={n} />
              ))}
            </datalist>
          </label>
          <label className="contact-field">
            <span>Telefono</span>
            <input type="tel" value={draft.phone} onChange={set("phone")} placeholder="Facoltativo" />
          </label>
          <label className="contact-field contact-field-notes">
            <span>Note</span>
            <textarea rows={3} value={draft.notes} onChange={set("notes")} placeholder="Facoltative" />
          </label>
          {error && <div className="contact-card-error">{error}</div>}
          <div className="contact-card-actions">
            <button type="button" className="btn btn-small" onClick={close}>
              Annulla
            </button>
            <button type="submit" className="btn btn-primary btn-small" disabled={saving}>
              {saving ? <LoaderCircle className="spin" /> : <Check />}
              Salva
            </button>
          </div>
        </form>
      )}
    </MorphWindow>
  );
}

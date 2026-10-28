import { useState, type FormEvent } from "react";
import { Building2, Check, LoaderCircle } from "lucide-react";
import { errorText, type CompanyInput } from "../api";
import { MorphWindow } from "./MorphWindow";

interface Props {
  /** Pulsante da cui la finestra nasce e in cui torna chiudendosi. */
  anchor: HTMLElement;
  initial: CompanyInput;
  /** Se rifiuta, l'errore resta nella finestra. */
  onSave: (company: CompanyInput) => Promise<unknown>;
  onClose: () => void;
}

/** Finestrella con i dati della scheda di un'azienda. */
export function CompanyCard({ anchor, initial, onSave, onClose }: Props) {
  const [draft, setDraft] = useState(initial);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  const set = (key: keyof CompanyInput) => (e: { target: { value: string } }) =>
    setDraft((d) => ({ ...d, [key]: e.target.value }));

  const submit = async (e: FormEvent, close: () => void) => {
    e.preventDefault();
    if (saving) return;
    setSaving(true);
    setError("");
    try {
      await onSave(draft);
      close();
    } catch (err) {
      setError(errorText(err));
      setSaving(false);
    }
  };

  return (
    <MorphWindow anchor={anchor} label="Scheda dell'azienda" onClose={onClose}>
      {(close) => (
        <form onSubmit={(e) => void submit(e, close)}>
          <div className="contact-card-title">
            <Building2 />
            Scheda dell'azienda
          </div>
          <label className="contact-field">
            <span>Nome</span>
            <input autoFocus value={draft.name} onChange={set("name")} required />
          </label>
          <label className="contact-field">
            <span>Partita IVA</span>
            <input value={draft.vat} onChange={set("vat")} placeholder="Facoltativa" />
          </label>
          <label className="contact-field">
            <span>Telefono</span>
            <input type="tel" value={draft.phone} onChange={set("phone")} placeholder="Facoltativo" />
          </label>
          <label className="contact-field contact-field-notes">
            <span>Indirizzo</span>
            <textarea rows={2} value={draft.address} onChange={set("address")} placeholder="Facoltativo" />
          </label>
          <label className="contact-field">
            <span>Sito web</span>
            <input value={draft.website} onChange={set("website")} placeholder="Facoltativo" />
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

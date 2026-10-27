import { useEffect, useRef } from "react";
import { Puzzle, RotateCw, X } from "lucide-react";
import { api, errorText, type AddonHook, type AddonHookType, type InstalledAddon } from "../api";

/** Un punto di aggancio di un add-on attivo. */
export interface AddonEntry {
  /** `<id>:<indice del punto di aggancio>` */
  key: string;
  addon: InstalledAddon;
  hook: AddonHook;
}

export function addonEntries(addons: InstalledAddon[], type: AddonHookType): AddonEntry[] {
  return addons.flatMap((addon) =>
    addon.enabled && addon.manifest
      ? addon.manifest.hooks.flatMap((hook, i) => (hook.type === type ? [{ key: `${addon.id}:${i}`, addon, hook }] : []))
      : [],
  );
}

/** Messaggio a cui è legato un add-on aperto dal pulsante sul messaggio. */
export interface AddonMessageRef {
  accountId: string;
  folder: string;
  uid: number;
}

export interface AddonDraft {
  to: string;
  cc: string;
  subject: string;
  body: string;
}

type ToastKind = "info" | "warn" | "error";

interface FrameProps {
  addon: InstalledAddon;
  hook: AddonHook;
  message: AddonMessageRef | null;
  onCompose: (draft: AddonDraft) => void;
  onReply: (body: string, all: boolean) => Promise<void>;
  onToast: (text: string, kind: ToastKind) => void;
  onClose: () => void;
}

function text(value: unknown, field: string, max: number): string {
  if (value === undefined || value === null) return "";
  if (typeof value !== "string") throw new Error(`${field} deve essere testo`);
  if (value.length > max) throw new Error(`${field} è troppo lungo`);
  return value;
}

/**
 * Pagina di un add-on, in un iframe con origine opaca (sandbox senza
 * allow-same-origin): non vede l'interfaccia né i comandi di Postamon. Le sue
 * richieste arrivano qui con postMessage (sdk/postamon-addon.js); si accettano
 * solo dal suo riquadro e solo per i permessi concessi.
 */
export function AddonFrame(props: FrameProps) {
  const frame = useRef<HTMLIFrameElement>(null);
  // Le richieste leggono sempre le props più recenti.
  const latest = useRef(props);
  latest.current = props;

  useEffect(() => {
    const handle = async (method: string, params: Record<string, unknown>): Promise<unknown> => {
      const { addon, hook, message, onCompose, onReply, onToast, onClose } = latest.current;
      const name = addon.manifest?.name ?? addon.id;
      const need = (permission: string) => {
        if (!addon.granted.includes(permission)) throw new Error(`L'add-on non ha il permesso «${permission}»`);
      };
      const needMessage = () => {
        if (!message) throw new Error("Qui non c'è un messaggio aperto");
        return message;
      };
      switch (method) {
        case "context":
          return {
            hook: hook.type,
            addon: { id: addon.id, version: addon.version },
            api: 1,
            message: message !== null,
          };
        case "message.read": {
          const m = needMessage();
          return api.addonMessage(addon.id, m.accountId, m.folder, m.uid);
        }
        case "compose.open":
          need("compose");
          onCompose({
            to: text(params.to, "to", 4000),
            cc: text(params.cc, "cc", 4000),
            subject: text(params.subject, "subject", 1000),
            body: text(params.body, "body", 200_000),
          });
          return null;
        case "compose.reply":
          need("compose");
          needMessage();
          await onReply(text(params.body, "body", 200_000), params.all === true);
          return null;
        case "storage.get":
          return api.addonStorage(addon.id, "get", text(params.key, "key", 100));
        case "storage.set":
          return api.addonStorage(addon.id, "set", text(params.key, "key", 100), params.value ?? null);
        case "storage.remove":
          return api.addonStorage(addon.id, "remove", text(params.key, "key", 100));
        case "storage.keys":
          return api.addonStorage(addon.id, "keys");
        case "ui.toast": {
          const kind: ToastKind = params.kind === "warn" || params.kind === "error" ? params.kind : "info";
          onToast(`${name}: ${text(params.text, "text", 300)}`, kind);
          return null;
        }
        case "ui.close":
          onClose();
          return null;
        default:
          throw new Error(`Funzione sconosciuta: ${method}`);
      }
    };

    const onMessage = (e: MessageEvent) => {
      const win = frame.current?.contentWindow;
      if (!win || e.source !== win) return;
      const req = e.data as { postamon?: unknown; id?: unknown; method?: unknown; params?: unknown } | null;
      if (!req || req.postamon !== 1 || typeof req.id !== "number" || typeof req.method !== "string") return;
      const params = req.params && typeof req.params === "object" ? (req.params as Record<string, unknown>) : {};
      // L'origine del riquadro è opaca ("null"): si risponde alla sua finestra, con "*".
      handle(req.method, params).then(
        (result) => win.postMessage({ postamon: 1, id: req.id, result: result ?? null }, "*"),
        (err) => win.postMessage({ postamon: 1, id: req.id, error: errorText(err) }, "*"),
      );
    };
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, []);

  // Mai allow-same-origin qui. Su Windows WebView2 inietta lo script di Tauri
  // anche nei riquadri, quindi `invoke` esiste pure dentro l'add-on: le sue
  // richieste vengono scartate solo perché l'origine opaca arriva come
  // `Origin: null`, che Tauri non accetta (provato: nessun effetto).
  return (
    <iframe
      ref={frame}
      className="addon-frame"
      title={`${props.addon.manifest?.name ?? props.addon.id}: ${props.hook.title}`}
      src={props.addon.baseUrl + props.hook.page}
      sandbox="allow-scripts allow-forms"
      allow="clipboard-write"
      referrerPolicy="no-referrer"
    />
  );
}

/** Icona dell'add-on (dal suo pacchetto) o quella generica. */
export function AddonIcon({ addon, size = 16 }: { addon: InstalledAddon | null; size?: number }) {
  const icon = addon?.manifest?.icon;
  if (!addon || !icon) return <Puzzle width={size} height={size} />;
  return <img className="addon-icon" src={addon.baseUrl + icon} alt="" width={size} height={size} />;
}

interface PanelProps extends Omit<FrameProps, "onClose"> {
  reloadKey: number;
  onReload: () => void;
  onClose: () => void;
}

/** Add-on aperto dal pulsante sul messaggio, in una finestra sopra la posta. */
export function AddonPanel({ reloadKey, onReload, ...frameProps }: PanelProps) {
  const { addon, hook, onClose } = frameProps;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="overlay">
      <div className="dialog addon-panel">
        <div className="dialog-head">
          <AddonIcon addon={addon} />
          <h3>
            {hook.title}
            <span className="addon-by">{addon.manifest?.name ?? addon.id}</span>
          </h3>
          {addon.dev && (
            <button className="btn btn-small" onClick={onReload}>
              <RotateCw />
              Ricarica
            </button>
          )}
          <button className="btn btn-ghost icon-btn" onClick={onClose} aria-label="Chiudi">
            <X />
          </button>
        </div>
        <AddonFrame key={reloadKey} {...frameProps} />
      </div>
    </div>
  );
}

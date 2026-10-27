import { useCallback, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  BookOpen,
  CircleCheck,
  FolderOpen,
  Globe,
  LoaderCircle,
  PackagePlus,
  Puzzle,
  RefreshCw,
  RotateCw,
  ShieldCheck,
  Trash,
  TriangleAlert,
  X,
} from "lucide-react";
import { api, errorText, permissionText, type CatalogItem, type InstalledAddon } from "../api";
import { AddonIcon } from "./AddonFrame";
import { Help } from "./Help";

const GUIDE_URL = "https://github.com/Xp67/Postamon/blob/main/ADDONS.md";
const DEV_KEY = "postamon.addon-dev";

function readDevMode(): boolean {
  try {
    return localStorage.getItem(DEV_KEY) === "1";
  } catch {
    return false;
  }
}

function writeDevMode(on: boolean) {
  try {
    localStorage.setItem(DEV_KEY, on ? "1" : "0");
  } catch {
    // archivio del browser non disponibile: si riparte spenta
  }
}

interface Props {
  installed: InstalledAddon[];
  /** Ricarica gli add-on installati; `frames` ricarica anche le pagine aperte. */
  onChanged: (frames?: boolean) => Promise<void>;
  onToast: (text: string, kind?: "info" | "warn" | "error") => void;
  onClose: () => void;
}

/** Marketplace degli add-on: catalogo firmato e add-on installati. */
export function AddonsDialog({ installed, onChanged, onToast, onClose }: Props) {
  const [tab, setTab] = useState<"catalog" | "installed">("catalog");
  const [catalog, setCatalog] = useState<CatalogItem[] | null>(null);
  const [loadError, setLoadError] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [consent, setConsent] = useState<CatalogItem | null>(null);
  const [removing, setRemoving] = useState<string | null>(null);
  const [devMode, setDevMode] = useState(readDevMode);

  const load = useCallback(async () => {
    setLoadError("");
    setCatalog(null);
    try {
      setCatalog(await api.addonsCatalog());
    } catch (e) {
      setLoadError(errorText(e));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (consent) setConsent(null);
      else onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [consent, onClose]);

  const install = async (item: CatalogItem) => {
    setBusy(item.id);
    try {
      await api.addonInstall(item.id, item.permissions);
      await onChanged(true);
      setCatalog((list) =>
        list?.map((c) =>
          c.id === item.id ? { ...c, installedVersion: item.version, update: false, newPermissions: [] } : c,
        ) ?? null,
      );
      setConsent(null);
      onToast(item.installedVersion ? `${item.name} aggiornato` : `${item.name} installato`);
    } catch (e) {
      onToast(errorText(e), "error");
    } finally {
      setBusy(null);
    }
  };

  // Nuova installazione, o aggiornamento che chiede permessi in più: prima si
  // mostra cosa potrà fare. Un aggiornamento con gli stessi permessi va diretto.
  const ask = (item: CatalogItem) => {
    if (item.installedVersion && item.newPermissions.length === 0) void install(item);
    else setConsent(item);
  };

  const uninstall = async (addon: InstalledAddon) => {
    setRemoving(null);
    setBusy(addon.id);
    try {
      await api.addonUninstall(addon.id);
      await onChanged();
      setCatalog((list) =>
        list?.map((c) => (c.id === addon.id ? { ...c, installedVersion: null, dev: false, update: false, newPermissions: c.permissions } : c)) ??
        null,
      );
      onToast(`${addon.manifest?.name ?? addon.id} disinstallato`);
    } catch (e) {
      onToast(errorText(e), "error");
    } finally {
      setBusy(null);
    }
  };

  const setEnabled = async (addon: InstalledAddon, enabled: boolean) => {
    try {
      await api.addonSetEnabled(addon.id, enabled);
      await onChanged();
    } catch (e) {
      onToast(errorText(e), "error");
    }
  };

  const loadFolder = async () => {
    const path = await open({ directory: true, title: "Cartella dell'add-on (quella con addon.json)" });
    if (typeof path !== "string") return;
    try {
      const id = await api.addonDevAdd(path);
      await onChanged(true);
      setTab("installed");
      onToast(`${id} caricato in modalità sviluppatore`);
    } catch (e) {
      onToast(errorText(e), "error");
    }
  };

  const installedCount = installed.length;

  return (
    <div className="overlay">
      <div className="dialog addons-dialog">
        <div className="dialog-head">
          <Puzzle />
          <h3>Add-on</h3>
          <div className="tabs" role="tablist">
            <button role="tab" aria-selected={tab === "catalog"} className={tab === "catalog" ? "active" : ""} onClick={() => setTab("catalog")}>
              Catalogo
            </button>
            <button
              role="tab"
              aria-selected={tab === "installed"}
              className={tab === "installed" ? "active" : ""}
              onClick={() => setTab("installed")}
            >
              Installati{installedCount > 0 ? ` (${installedCount})` : ""}
            </button>
          </div>
          <button className="btn btn-ghost icon-btn" onClick={onClose} aria-label="Chiudi">
            <X />
          </button>
        </div>

        <div className="addons-body">
          {consent ? (
            <Consent item={consent} busy={busy === consent.id} onConfirm={() => void install(consent)} onCancel={() => setConsent(null)} />
          ) : tab === "catalog" ? (
            catalog === null && !loadError ? (
              <div className="empty addons-empty">
                <LoaderCircle className="spin" />
              </div>
            ) : loadError ? (
              <div className="empty addons-empty">
                <div>
                  <TriangleAlert />
                  <div>{loadError}</div>
                  <button className="btn" style={{ marginTop: 14 }} onClick={() => void load()}>
                    <RefreshCw />
                    Riprova
                  </button>
                </div>
              </div>
            ) : catalog!.length === 0 ? (
              <div className="empty addons-empty">
                <div>
                  <Puzzle />
                  <div>Nel catalogo non ci sono ancora add-on.</div>
                </div>
              </div>
            ) : (
              catalog!.map((item) => (
                <div className="addon-row" key={item.id}>
                  <span className="addon-row-icon">
                    <Puzzle />
                  </span>
                  <div className="addon-main">
                    <div className="addon-name">
                      {item.name}
                      <span className="addon-meta">
                        {item.version} · {item.author}
                      </span>
                    </div>
                    <div className="addon-desc">{item.description}</div>
                    <PermissionChips permissions={item.permissions} />
                  </div>
                  <div className="addon-actions">
                    {item.incompatible ? (
                      <span className="tag warn">{item.incompatible}</span>
                    ) : item.dev ? (
                      <span className="tag">Caricato in sviluppo</span>
                    ) : item.update ? (
                      <button className="btn btn-primary btn-small" disabled={busy !== null} onClick={() => ask(item)}>
                        {busy === item.id ? <LoaderCircle className="spin" /> : <RefreshCw />}
                        Aggiorna a {item.version}
                      </button>
                    ) : item.installedVersion ? (
                      <span className="tag ok">
                        <CircleCheck />
                        Installato
                      </span>
                    ) : (
                      <button className="btn btn-primary btn-small" disabled={busy !== null} onClick={() => ask(item)}>
                        <PackagePlus />
                        Installa
                      </button>
                    )}
                  </div>
                </div>
              ))
            )
          ) : installedCount === 0 ? (
            <div className="empty addons-empty">
              <div>
                <Puzzle />
                <div>Nessun add-on installato.</div>
              </div>
            </div>
          ) : (
            installed.map((addon) => {
              const name = addon.manifest?.name ?? addon.id;
              return (
                <div className={`addon-row ${addon.enabled ? "" : "off"}`} key={addon.id}>
                  <span className="addon-row-icon">
                    <AddonIcon addon={addon} size={20} />
                  </span>
                  <div className="addon-main">
                    <div className="addon-name">
                      {name}
                      <span className="addon-meta">{addon.version}</span>
                      {addon.dev && <span className="tag">In sviluppo</span>}
                    </div>
                    {addon.error ? (
                      <div className="addon-desc addon-error">
                        <TriangleAlert />
                        {addon.error}
                      </div>
                    ) : (
                      <div className="addon-desc">{addon.manifest?.description}</div>
                    )}
                    {addon.devPath && <div className="addon-path selectable">{addon.devPath}</div>}
                    <PermissionChips permissions={addon.granted} />
                  </div>
                  <div className="addon-actions">
                    <label className="switch">
                      <input
                        type="checkbox"
                        checked={addon.enabled}
                        onChange={(e) => void setEnabled(addon, e.target.checked)}
                      />
                      Attivo
                    </label>
                    {addon.dev && (
                      <button className="btn btn-small" onClick={() => void onChanged(true)}>
                        <RotateCw />
                        Ricarica
                      </button>
                    )}
                    {removing === addon.id ? (
                      <button className="btn btn-danger btn-small" onClick={() => void uninstall(addon)}>
                        <Trash />
                        Conferma
                      </button>
                    ) : (
                      <button className="btn btn-danger btn-small" disabled={busy !== null} onClick={() => setRemoving(addon.id)}>
                        {busy === addon.id ? <LoaderCircle className="spin" /> : <Trash />}
                        {addon.dev ? "Togli" : "Disinstalla"}
                      </button>
                    )}
                  </div>
                </div>
              );
            })
          )}
        </div>

        <div className="addons-foot">
          <label className="switch">
            <input
              type="checkbox"
              checked={devMode}
              onChange={(e) => {
                setDevMode(e.target.checked);
                writeDevMode(e.target.checked);
              }}
            />
            Modalità sviluppatore
          </label>
          <Help align="left">
            Per chi scrive add-on: carica un add-on da una cartella del PC, senza catalogo né firma, e lo ricarica
            quando lo modifichi. Le regole di sicurezza restano le stesse degli add-on installati.
          </Help>
          {devMode && (
            <button className="btn btn-small" onClick={() => void loadFolder()}>
              <FolderOpen />
              Carica da una cartella
            </button>
          )}
          <span className="spacer" />
          <button className="btn btn-ghost btn-small" onClick={() => void openUrl(GUIDE_URL)}>
            <BookOpen />
            Scrivi un add-on
          </button>
        </div>
      </div>
    </div>
  );
}

function PermissionChips({ permissions }: { permissions: string[] }) {
  if (permissions.length === 0) return null;
  return (
    <div className="addon-perms">
      {permissions.map((p) => {
        const { text, warn } = permissionText(p);
        return (
          <span key={p} className={`perm ${warn ? "warn" : ""}`} title={text}>
            {warn ? <Globe /> : <ShieldCheck />}
            {p.startsWith("network:") ? p.slice(8) : p === "message.read" ? "Legge il messaggio" : p === "compose" ? "Prepara risposte" : p}
          </span>
        );
      })}
    </div>
  );
}

function Consent({
  item,
  busy,
  onConfirm,
  onCancel,
}: {
  item: CatalogItem;
  busy: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const update = item.installedVersion !== null;
  const hosts = item.permissions.filter((p) => p.startsWith("network:")).map((p) => p.slice(8));
  const readsAndSends = item.permissions.includes("message.read") && hosts.length > 0;
  return (
    <div className="addon-consent">
      <h4>
        {update ? `Aggiornare ${item.name} alla versione ${item.version}?` : `Installare ${item.name}?`}
      </h4>
      <div className="addon-meta">
        di {item.author} · versione {item.version}
      </div>
      <div className="consent-title">
        {item.permissions.length === 0 ? "Non chiede permessi speciali." : update ? "La nuova versione potrà:" : "Potrà:"}
        <Help>
          Gli add-on girano in un riquadro isolato: non vedono le password, le altre caselle, gli altri messaggi né i
          file del PC, e non si collegano a internet se non è scritto qui. Possono sempre salvare i propri dati, che si
          cancellano quando li disinstalli.
        </Help>
      </div>
      {item.permissions.length > 0 && (
        <ul className="consent-list">
          {item.permissions.map((p) => {
            const { text, warn } = permissionText(p);
            const isNew = update && item.newPermissions.includes(p);
            return (
              <li key={p} className={warn ? "warn" : ""}>
                {warn ? <Globe /> : <ShieldCheck />}
                <span>{text}</span>
                {isNew && <span className="tag">Nuovo</span>}
              </li>
            );
          })}
        </ul>
      )}
      {readsAndSends && (
        <div className="compose-warning">
          <TriangleAlert />
          <span>
            Legge i tuoi messaggi e si collega a {hosts.join(", ")}: installalo solo se ti fidi di chi l'ha scritto.
          </span>
        </div>
      )}
      <div className="consent-actions">
        <button className="btn btn-primary" onClick={onConfirm} disabled={busy}>
          {busy ? <LoaderCircle className="spin" /> : update ? <RefreshCw /> : <PackagePlus />}
          {update ? "Aggiorna" : "Installa"}
        </button>
        <button className="btn" onClick={onCancel} disabled={busy}>
          Annulla
        </button>
      </div>
    </div>
  );
}

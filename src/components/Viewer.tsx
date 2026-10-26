import { useEffect, useState } from "react";
import { Download, ExternalLink, LoaderCircle, PackagePlus, TriangleAlert, X } from "lucide-react";
import type { Attachment, Preview, WebProgress } from "../api";
import { fileSize } from "../format";

export interface ViewerState {
  attachment: Attachment;
  preview: Preview | null;
  progress: WebProgress | null;
  error: string;
}

interface Props {
  state: ViewerState;
  craftMissing: boolean;
  onClose: () => void;
  onSave: () => void;
  onOpenNative: () => void;
  onOpenSystem: () => void;
  onInstallCraft: () => void;
}

/** Allegato aperto dentro Postamon con l'app ArtCraft adatta, nella sua versione web. */
export function Viewer({ state, craftMissing, onClose, onSave, onOpenNative, onOpenSystem, onInstallCraft }: Props) {
  const { attachment, preview, progress, error } = state;
  const [bridge, setBridge] = useState<"" | "opened" | "timeout" | "error">("");

  // Lo script del server delle anteprime avvisa quando il file è arrivato all'app.
  useEffect(() => {
    if (!preview) return;
    const onMessage = (e: MessageEvent) => {
      if (e.origin !== preview.origin) return;
      const s = (e.data as { postamon?: string } | null)?.postamon;
      if (s === "opened" || s === "timeout" || s === "error") setBridge(s);
    };
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, [preview]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const appName = preview?.appName ?? progress?.appName ?? "";

  return (
    <div className="overlay viewer-overlay">
      <div className="dialog viewer">
        <div className="dialog-head">
          <h3>
            {appName && <span className="viewer-app">{appName}</span>}
            {attachment.name}
          </h3>
          {preview?.native && (
            <button className="btn" onClick={onOpenNative}>
              <ExternalLink />
              Apri in {preview.appName}
            </button>
          )}
          {preview && !preview.native && craftMissing && (
            <button className="btn" onClick={onInstallCraft}>
              <PackagePlus />
              Installa ArtCraft
            </button>
          )}
          <button className="btn" onClick={onSave}>
            <Download />
            Salva
          </button>
          <button className="btn btn-ghost icon-btn" onClick={onClose} aria-label="Chiudi">
            <X />
          </button>
        </div>

        {(bridge === "timeout" || bridge === "error") && (
          <div className="compose-warning">
            <TriangleAlert />
            <span style={{ flex: 1 }}>{appName} non ha aperto il file da solo.</span>
            <button className="btn btn-small" onClick={onOpenSystem}>
              Apri con il programma predefinito
            </button>
          </div>
        )}

        {preview ? (
          <iframe
            className="viewer-frame"
            title={`${preview.appName}: ${attachment.name}`}
            src={preview.url}
            sandbox="allow-scripts allow-same-origin allow-downloads allow-popups allow-forms allow-modals"
            allow="fullscreen; clipboard-read; clipboard-write"
          />
        ) : error ? (
          <div className="empty">
            <div>
              <TriangleAlert />
              <div>{error}</div>
              <button className="btn" style={{ marginTop: 14 }} onClick={onOpenSystem}>
                Apri con il programma predefinito
              </button>
            </div>
          </div>
        ) : (
          <div className="empty">
            <div className="viewer-progress">
              <LoaderCircle className="spin" />
              <div>{appName ? `Preparo ${appName}…` : "Preparo l'anteprima…"}</div>
              {progress && progress.total > 0 && (
                <>
                  <div className="bar">
                    <span style={{ width: `${Math.min(100, (progress.received / progress.total) * 100)}%` }} />
                  </div>
                  <div className="bar-text">
                    {fileSize(progress.received)} di {fileSize(progress.total)}, solo la prima volta
                  </div>
                </>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

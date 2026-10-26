import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { CircleCheck, LoaderCircle, PackagePlus, Sparkles, TriangleAlert, X } from "lucide-react";
import { api, errorText, type CraftStatus, type SetupReport, type SetupStep } from "../api";
import { fileSize } from "../format";
import { Help } from "./Help";

interface Props {
  status: CraftStatus;
  onClose: (installed: boolean) => void;
}

/** Richiesta di installare le app ArtCraft che mancano sul PC. */
export function ArtCraftSetup({ status, onClose }: Props) {
  const { missing } = status;
  const [step, setStep] = useState<SetupStep | null>(null);
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<SetupReport | null>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    const unlisten = listen<SetupStep>("craft-setup", ({ payload }) => setStep(payload));
    return () => void unlisten.then((f) => f());
  }, []);

  const install = async () => {
    setBusy(true);
    setError("");
    try {
      setReport(await api.craftInstall());
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const allNew = missing.length === status.total;
  const done = report !== null;
  const ok = done && !report.cancelled && report.failed.length === 0;

  return (
    <div className="overlay">
      <div className="dialog small">
        <div className="dialog-head">
          <h3>Installa ArtCraft</h3>
          {!busy && (
            <button className="btn btn-ghost icon-btn" onClick={() => onClose(ok)} aria-label="Chiudi">
              <X />
            </button>
          )}
        </div>
        <div className="dialog-body">
          {!done && (
            <>
              <p className="recommend">
                <Sparkles />
                Caldamente raccomandato per l'utilizzo corretto di Postamon.
              </p>
              <p className="setup-lead">
                Con le app ArtCraft vedi e modifichi gli allegati (PDF, foto, documenti, fogli di calcolo,
                presentazioni, disegni) direttamente dentro Postamon.
              </p>
              <div className="setup-what">
                {allNew ? `${missing.length} programmi ArtCraft` : `Mancano: ${missing.join(", ")}`}
                <Help>
                  Programmi gratuiti e open source di ArtCraft, scaricati dalle loro pubblicazioni ufficiali e
                  controllati prima di installarli. Non lasciano icone sul desktop. Windows chiede una sola volta la
                  conferma di amministratore.
                </Help>
              </div>
            </>
          )}

          {busy && step && (
            <div className="viewer-progress setup-progress">
              <LoaderCircle className="spin" />
              {step.stage === "download" ? (
                <>
                  <div>
                    Scarico {step.app} ({step.index + 1} di {step.count})
                  </div>
                  {step.total > 0 && (
                    <>
                      <div className="bar">
                        <span style={{ width: `${Math.min(100, (step.received / step.total) * 100)}%` }} />
                      </div>
                      <div className="bar-text">
                        {fileSize(step.received)} di {fileSize(step.total)}
                      </div>
                    </>
                  )}
                </>
              ) : (
                <div>Installo i programmi: conferma la richiesta di Windows…</div>
              )}
            </div>
          )}

          {done && (
            <div className="setup-result">
              {ok ? (
                <p className="recommend ok">
                  <CircleCheck />
                  ArtCraft è installato.
                </p>
              ) : report.cancelled ? (
                <p className="recommend warn">
                  <TriangleAlert />
                  Installazione annullata: la richiesta di Windows non è stata confermata.
                </p>
              ) : (
                <>
                  <p className="recommend warn">
                    <TriangleAlert />
                    Alcuni programmi non sono stati installati.
                  </p>
                  <ul className="setup-failed">
                    {report.failed.map((f) => (
                      <li key={f}>{f}</li>
                    ))}
                  </ul>
                </>
              )}
            </div>
          )}

          {error && <div className="form-error">{error}</div>}

          <div className="card-actions" style={{ marginTop: 18 }}>
            {!ok && (
              <button className="btn btn-primary" onClick={() => void install()} disabled={busy}>
                {busy ? <LoaderCircle className="spin" /> : <PackagePlus />}
                {busy ? "Installazione in corso…" : done ? "Riprova" : "Installa ArtCraft"}
              </button>
            )}
            {!busy && (
              <button className={ok ? "btn btn-primary" : "btn"} onClick={() => onClose(ok)}>
                {ok ? "Fatto" : "Non ora"}
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

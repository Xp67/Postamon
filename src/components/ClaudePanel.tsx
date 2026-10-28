import { useCallback, useEffect, useRef, useState, type KeyboardEvent } from "react";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  ArrowLeft,
  ArrowUp,
  CircleCheck,
  CircleX,
  CloudUpload,
  Download,
  LoaderCircle,
  LogIn,
  Mail,
  RefreshCw,
  Search,
  Settings,
  ShieldCheck,
  Sparkles,
  Square,
  SquarePen,
  TriangleAlert,
  X,
} from "lucide-react";
import { api, CLAUDE_MODELS, claudeToolText, errorText, type ClaudeChatEvent, type ClaudeStatus } from "../api";
import { Help } from "./Help";
import { Markdown } from "./Markdown";

const SETUP_URL = "https://code.claude.com/docs/en/setup";

type ToastKind = "info" | "warn" | "error";

type Segment =
  | { type: "text"; text: string }
  | { type: "tool"; id: string; name: string; input: Record<string, unknown> | null; state: "run" | "ok" | "error" };

interface Turn {
  id: number;
  role: "user" | "assistant";
  text: string;
  segments: Segment[];
  error: string | null;
  stopped: boolean;
}

interface Props {
  open: boolean;
  status: ClaudeStatus | null;
  onRefresh: () => Promise<ClaudeStatus | null>;
  /** Oggetto del messaggio aperto, per le proposte rapide. */
  openSubject: string | null;
  onToast: (text: string, kind?: ToastKind) => void;
  onClose: () => void;
}

function planLabel(cli: NonNullable<ClaudeStatus["cli"]>): string {
  const plan = cli.plan?.toLowerCase();
  if (plan) return plan.charAt(0).toUpperCase() + plan.slice(1);
  return cli.authMethod === "claude.ai" ? "" : "a consumo";
}

/**
 * Pannello «Claude»: una conversazione con il Claude Code del PC, che usa la
 * posta con gli strumenti di Postamon. Resta montato anche da chiuso, così
 * una risposta in corso non si perde.
 */
export function ClaudePanel({ open, status, onRefresh, openSubject, onToast, onClose }: Props) {
  const [showSettings, setShowSettings] = useState(false);
  const [turns, setTurns] = useState<Turn[]>([]);
  const [running, setRunning] = useState(false);
  const [draft, setDraft] = useState("");
  const [limitAt, setLimitAt] = useState<number | null>(null);
  const [waitingLogin, setWaitingLogin] = useState(false);
  const [enabling, setEnabling] = useState(false);
  const [model, setModel] = useState(CLAUDE_MODELS[0].id);
  // Versione esatta del modello, come la dice Claude Code all'avvio.
  const [modelName, setModelName] = useState("");
  // Eventi con un numero più basso vengono da conversazioni fermate.
  const floor = useRef(0);
  const generation = useRef(0);
  const nextId = useRef(1);
  const messages = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLTextAreaElement>(null);

  const updateLast = useCallback((change: (t: Turn) => Turn) => {
    setTurns((list) => {
      const last = list[list.length - 1];
      if (!last || last.role !== "assistant") return list;
      return [...list.slice(0, -1), change(last)];
    });
  }, []);

  useEffect(() => {
    const unlisten = listen<ClaudeChatEvent>("claude-chat", ({ payload: e }) => {
      if (e.generation < floor.current) return;
      switch (e.kind) {
        case "ready":
          setModelName(e.model);
          break;
        case "text":
          updateLast((t) => {
            const segments = [...t.segments];
            const last = segments[segments.length - 1];
            if (last?.type === "text") segments[segments.length - 1] = { ...last, text: last.text + e.delta };
            else segments.push({ type: "text", text: e.delta });
            return { ...t, segments };
          });
          break;
        case "tool":
          updateLast((t) => ({
            ...t,
            segments: [...t.segments, { type: "tool", id: e.id, name: e.name, input: e.input, state: "run" }],
          }));
          break;
        case "toolResult":
          updateLast((t) => ({
            ...t,
            segments: t.segments.map((s) =>
              s.type === "tool" && s.id === e.id ? { ...s, state: e.isError ? "error" : "ok" } : s,
            ),
          }));
          break;
        case "limit":
          setLimitAt(e.resetsAt);
          break;
        case "done":
        case "exit":
          setRunning(false);
          if (e.error) updateLast((t) => (t.error ? t : { ...t, error: e.error }));
          break;
      }
    });
    return () => void unlisten.then((f) => f());
  }, [updateLast]);

  useEffect(() => {
    const el = messages.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [turns]);

  useEffect(() => {
    if (status?.model) setModel(status.model);
  }, [status?.model]);

  const changeModel = async (next: string) => {
    const previous = model;
    setModel(next);
    setModelName("");
    try {
      // Claude Code riparte con il nuovo modello al prossimo messaggio.
      floor.current = generation.current + 1;
      await api.claudeSetModel(next);
    } catch (e) {
      setModel(previous);
      onToast(errorText(e), "error");
    }
  };

  useEffect(() => {
    if (open && status?.enabled && !showSettings) input.current?.focus();
  }, [open, status?.enabled, showSettings]);

  // Dopo «Accedi» si aspetta che l'accesso finisca nella finestra di Claude Code.
  useEffect(() => {
    if (!waitingLogin) return;
    let tries = 0;
    const timer = window.setInterval(() => {
      tries += 1;
      void onRefresh().then((s) => {
        if (s?.cli?.loggedIn || tries > 100) setWaitingLogin(false);
      });
    }, 3000);
    return () => window.clearInterval(timer);
  }, [waitingLogin, onRefresh]);

  const send = async (text: string) => {
    const message = text.trim();
    if (!message || running) return;
    setDraft("");
    setLimitAt(null);
    setTurns((list) => [
      ...list,
      { id: nextId.current++, role: "user", text: message, segments: [], error: null, stopped: false },
      { id: nextId.current++, role: "assistant", text: "", segments: [], error: null, stopped: false },
    ]);
    setRunning(true);
    try {
      generation.current = await api.claudeChatSend(message);
    } catch (e) {
      setRunning(false);
      updateLast((t) => ({ ...t, error: errorText(e) }));
    }
  };

  const stop = async () => {
    floor.current = generation.current + 1;
    await api.claudeChatStop();
    setRunning(false);
    updateLast((t) => ({
      ...t,
      stopped: true,
      segments: t.segments.map((s) => (s.type === "tool" && s.state === "run" ? { ...s, state: "error" } : s)),
    }));
  };

  const reset = async () => {
    floor.current = generation.current + 1;
    await api.claudeChatReset();
    setRunning(false);
    setTurns([]);
    setLimitAt(null);
  };

  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void send(draft);
    }
  };

  const login = async () => {
    try {
      await api.claudeLogin();
      setWaitingLogin(true);
    } catch (e) {
      onToast(errorText(e), "error");
    }
  };

  const enable = async () => {
    setEnabling(true);
    try {
      await api.claudeSetEnabled(true);
      await onRefresh();
    } catch (e) {
      onToast(errorText(e), "error");
    } finally {
      setEnabling(false);
    }
  };

  const cli = status?.cli ?? null;
  const ready = !!status?.enabled && !!cli?.loggedIn;
  const suggestions =
    openSubject !== null
      ?["Riassumi questo messaggio", "Prepara una risposta a questo messaggio", "Cosa mi chiede questo messaggio?"]
    : ["Cosa c'è di nuovo nella posta in arrivo?", "Quali messaggi aspettano una mia risposta?", "Che add-on ho?"];

  let body;
  if (!status) {
    body = (
      <div className="claude-center">
        <LoaderCircle className="spin" />
      </div>
    );
  } else if (showSettings) {
    body = <ClaudeSettings status={status} onRefresh={onRefresh} onToast={onToast} onLogin={() => void login()} />;
  } else if (!cli) {
    body = (
      <div className="claude-setup">
        <Download className="claude-setup-icon" />
        <h3>
          Serve Claude Code
          <Help align="left">
            Il pannello usa Claude Code, il programma di Anthropic, con l'abbonamento Claude con cui accedi lì.
            Postamon non vede mai il tuo account.
          </Help>
        </h3>
        <p>Su questo PC Claude Code non c'è ancora.</p>
        <div className="claude-setup-actions">
          <button className="btn btn-primary" onClick={() => void openUrl(SETUP_URL)}>
            <Download />
            Come installarlo
          </button>
          <button className="btn" onClick={() => void onRefresh()}>
            <RefreshCw />
            Controlla di nuovo
          </button>
        </div>
      </div>
    );
  } else if (!cli.loggedIn) {
    body = (
      <div className="claude-setup">
        <LogIn className="claude-setup-icon" />
        <h3>
          Accedi a Claude
          <Help align="left">
            Si apre la finestra di Claude Code e poi la pagina di Anthropic, dove entri con il tuo abbonamento Claude.
            Postamon non vede la password né l'account: legge solo se l'accesso è fatto.
          </Help>
        </h3>
        {waitingLogin ? (
          <p className="claude-wait">
            <LoaderCircle className="spin" />
            Completa l'accesso nella finestra che si è aperta
          </p>
        ) : (
          <p>Claude Code è installato, ma senza accesso.</p>
        )}
        <div className="claude-setup-actions">
          <button className="btn btn-primary" onClick={() => void login()} disabled={waitingLogin}>
            <LogIn />
            Accedi
          </button>
        </div>
      </div>
    );
  } else if (!status.enabled) {
    body = (
      <div className="claude-setup">
        <Sparkles className="claude-setup-icon" />
        <h3>Claude nella tua posta</h3>
        <ul className="consent-list">
          <li>
            <Search />
            <span>Legge e cerca la posta di tutte le caselle di Postamon</span>
          </li>
          <li>
            <SquarePen />
            <span>Prepara messaggi e risposte, segna i messaggi, gestisce gli add-on</span>
          </li>
          <li>
            <ShieldCheck />
            <span>Invia, sposta, cestina e installa add-on solo dopo la tua conferma</span>
          </li>
          <li className="warn">
            <CloudUpload />
            <span>I messaggi che legge arrivano ad Anthropic, come in ogni conversazione con Claude</span>
          </li>
        </ul>
        <div className="claude-setup-actions">
          <button className="btn btn-primary" onClick={() => void enable()} disabled={enabling}>
            <Sparkles />
            Attiva
          </button>
        </div>
      </div>
    );
  } else {
    body = (
      <>
        <div className="chat-messages claude-messages" ref={messages}>
          {turns.length === 0 && (
            <div className="claude-intro">
              <Sparkles />
              <p>Chiedi a Claude della tua posta.</p>
              <div className="claude-suggestions">
                {suggestions.map((s) => (
                  <button key={s} className="btn btn-small" onClick={() => void send(s)}>
                    {s}
                  </button>
                ))}
              </div>
            </div>
          )}
          {turns.map((t) =>
            t.role === "user" ? (
              <div key={t.id} className="bubble-row mine">
                <div className="bubble">
                  <div className="bubble-text selectable">{t.text}</div>
                </div>
              </div>
            ) : (
              <div key={t.id} className="claude-answer selectable">
                {t.segments.map((s, i) =>
                  s.type === "text" ? (
                    <Markdown key={i} text={s.text} />
                  ) : (
                    <div key={s.id || i} className={`claude-tool ${s.state}`}>
                      {s.state === "run" ? (
                        <LoaderCircle className="spin" />
                      ) : s.state === "ok" ? (
                        <CircleCheck />
                      ) : (
                        <CircleX />
                      )}
                      {claudeToolText(s.name, s.input)}
                    </div>
                  ),
                )}
                {running && t === turns[turns.length - 1] && t.segments.length === 0 && (
                  <div className="claude-tool run">
                    <LoaderCircle className="spin" />
                    Ci penso…
                  </div>
                )}
                {t.stopped && <div className="claude-stopped">Risposta fermata</div>}
                {t.error && (
                  <div className="claude-error">
                    <TriangleAlert />
                    <span>{t.error}</span>
                  </div>
                )}
              </div>
            ),
          )}
          {limitAt !== null && (
            <div className="claude-error">
              <TriangleAlert />
              <span>
                Hai raggiunto il limite del tuo piano Claude
                {limitAt > 0 &&
                  `: riprende alle ${new Date(limitAt * 1000).toLocaleTimeString("it-IT", { hour: "2-digit", minute: "2-digit" })}`}
              </span>
            </div>
          )}
        </div>
        {openSubject !== null && (
          <div className="claude-context" title="Claude sa quale messaggio hai aperto">
            <Mail />
            <span>{openSubject || "(senza oggetto)"}</span>
          </div>
        )}
        <div className="chat-composer">
          <textarea
            ref={input}
            rows={1}
            value={draft}
            placeholder="Scrivi a Claude…"
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={onKey}
          />
          {running ? (
            <button className="btn icon-btn" onClick={() => void stop()} title="Ferma" aria-label="Ferma">
              <Square />
            </button>
          ) : (
            <button
              className="btn btn-primary icon-btn"
              onClick={() => void send(draft)}
              disabled={!draft.trim()}
              title="Invia a Claude"
              aria-label="Invia a Claude"
            >
              <ArrowUp />
            </button>
          )}
        </div>
      </>
    );
  }

  return (
    <aside className="sheet claude-panel" hidden={!open} aria-label="Claude">
      <div className="claude-head">
        <Sparkles className="claude-mark" />
        <h2>Claude</h2>
        {cli?.loggedIn && (
          <span className="claude-who" title={cli.email ?? ""}>
            {[cli.email, planLabel(cli)].filter(Boolean).join(" · ")}
          </span>
        )}
        <span className="spacer" />
        {ready && (
          <select
            className="claude-model"
            value={model}
            disabled={running}
            onChange={(e) => void changeModel(e.target.value)}
            title={running ? "Si cambia a risposta finita" : modelName ? `Modello: ${modelName}` : "Modello di Claude"}
            aria-label="Modello di Claude"
          >
            {CLAUDE_MODELS.map((m) => (
              <option key={m.id} value={m.id}>
                {m.label}
              </option>
            ))}
          </select>
        )}
        {ready && !showSettings && turns.length > 0 && (
          <button
            className="btn btn-ghost icon-btn"
            onClick={() => void reset()}
            title="Nuova conversazione"
            aria-label="Nuova conversazione"
          >
            <SquarePen />
          </button>
        )}
        {status && cli && (
          <button
            className={`btn btn-ghost icon-btn ${showSettings ? "active" : ""}`}
            onClick={() => setShowSettings(!showSettings)}
            title={showSettings ? "Torna alla conversazione" : "Impostazioni"}
            aria-label={showSettings ? "Torna alla conversazione" : "Impostazioni"}
          >
            {showSettings ? <ArrowLeft /> : <Settings />}
          </button>
        )}
        <button className="btn btn-ghost icon-btn" onClick={onClose} title="Chiudi" aria-label="Chiudi">
          <X />
        </button>
      </div>
      {body}
    </aside>
  );
}

function ClaudeSettings({
  status,
  onRefresh,
  onToast,
  onLogin,
}: {
  status: ClaudeStatus;
  onRefresh: () => Promise<ClaudeStatus | null>;
  onToast: (text: string, kind?: ToastKind) => void;
  onLogin: () => void;
}) {
  const [busy, setBusy] = useState<string | null>(null);
  const cli = status.cli;

  const run = async (key: string, action: () => Promise<void>, done?: string) => {
    setBusy(key);
    try {
      await action();
      await onRefresh();
      if (done) onToast(done);
    } catch (e) {
      onToast(errorText(e), "error");
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="claude-settings">
      <section>
        <label className="switch">
          <input
            type="checkbox"
            checked={status.enabled}
            disabled={busy !== null}
            onChange={(e) => void run("enabled", () => api.claudeSetEnabled(e.target.checked))}
          />
          Claude può usare la posta
        </label>
        <Help align="left">
          Spento, Claude non legge né tocca niente: né dal pannello né da Claude Desktop o Claude Code. Acceso, legge
          e organizza la posta; inviare, spostare, cestinare e installare o togliere add-on li confermi sempre qui.
        </Help>
      </section>

      <h4>Account</h4>
      <section className="claude-row">
        <div className="claude-row-main">
          <b>{cli?.loggedIn ? cli.email : "Senza accesso"}</b>
          <span>
            {cli ? `Claude Code ${cli.version}` : "Claude Code non installato"}
            {cli?.loggedIn && planLabel(cli) && ` · piano ${planLabel(cli)}`}
          </span>
        </div>
        {cli && !cli.loggedIn && (
          <button className="btn btn-small" onClick={onLogin}>
            <LogIn />
            Accedi
          </button>
        )}
      </section>

      <h4>
        Usa Postamon anche da
        <Help align="left">
          Collegato, puoi chiedere a Claude Desktop o a Claude Code di leggere e organizzare la posta di Postamon, con
          il tuo abbonamento. Se Postamon è chiuso si apre da solo; le azioni le confermi comunque qui.
        </Help>
      </h4>
      <section className="claude-row">
        <div className="claude-row-main">
          <b>Claude Desktop</b>
          <span>
            {!status.desktopInstalled ? "Non installato" : status.desktopLinked ? "Collegato" : "Non collegato"}
          </span>
        </div>
        {status.desktopInstalled && (
          <button
            className="btn btn-small"
            disabled={busy !== null}
            onClick={() =>
              void run(
                "desktop",
                () => api.claudeLinkDesktop(!status.desktopLinked),
                status.desktopLinked
                  ? "Postamon tolto da Claude Desktop"
                  : "Fatto: chiudi e riapri Claude Desktop per vedere Postamon",
              )
            }
          >
            {busy === "desktop" && <LoaderCircle className="spin" />}
            {status.desktopLinked ? "Scollega" : "Collega"}
          </button>
        )}
      </section>
      <section className="claude-row">
        <div className="claude-row-main">
          <b>Claude Code</b>
          <span>{!cli ? "Non installato" : status.codeLinked ? "Collegato" : "Non collegato"}</span>
        </div>
        {cli && (
          <button
            className="btn btn-small"
            disabled={busy !== null}
            onClick={() =>
              void run(
                "code",
                () => api.claudeLinkCode(!status.codeLinked),
                status.codeLinked ? "Postamon tolto da Claude Code" : "Fatto: Postamon è fra gli strumenti di Claude Code",
              )
            }
          >
            {busy === "code" && <LoaderCircle className="spin" />}
            {status.codeLinked ? "Scollega" : "Collega"}
          </button>
        )}
      </section>
    </div>
  );
}

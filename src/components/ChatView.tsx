import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { downloadDir, join } from "@tauri-apps/api/path";
import { open as openDialog, save } from "@tauri-apps/plugin-dialog";
import {
  Download,
  File as FileIcon,
  LoaderCircle,
  LogIn,
  LogOut,
  MessageSquarePlus,
  MessagesSquare,
  Paperclip,
  Search,
  SendHorizontal,
  Users,
  WifiOff,
  X,
} from "lucide-react";
import {
  api,
  errorText,
  type ChatConversation,
  type Account,
  type ChatEventFor,
  type ChatStatusFor,
  type ChatFile,
  type ChatMessage,
  type ChatUser,
  type FileInfo,
  type InternoStatus,
  type UploadProgress,
  type WebProgress,
} from "../api";
import { avatarHue, fileSize, initials } from "../format";
import { Help } from "./Help";
import { Viewer, type ViewerState } from "./Viewer";

interface Props {
  /** Casella dello spazio di lavoro: la chat entra con il suo indirizzo. */
  account: Account;
  /** Il trascinamento di file vale per la chat solo se non c'è una finestra sopra. */
  dropEnabled: boolean;
  onToast: (text: string, kind?: "info" | "warn" | "error") => void;
}

export function ChatView({ account, dropEnabled, onToast }: Props) {
  const [status, setStatus] = useState<InternoStatus | null>(null);

  const refresh = useCallback(() => {
    void api.internoStatus(account.id).then(setStatus);
  }, [account.id]);
  useEffect(refresh, [refresh]);

  if (!status) return null;
  if (!status.configured || !status.user) {
    return <InternoLogin account={account} status={status} onDone={refresh} />;
  }
  return (
    <ChatMain
      accountId={account.id}
      status={status}
      me={status.user.id}
      dropEnabled={dropEnabled}
      onToast={onToast}
      onLogout={refresh}
    />
  );
}

// ---- attivazione ----

function InternoLogin({ account, status, onDone }: { account: Account; status: InternoStatus; onDone: () => void }) {
  const [server, setServer] = useState(status.defaultServer);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError("");
    try {
      await api.internoLogin(account.id, server);
      onDone();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="chat-login">
      <form className="card" onSubmit={submit}>
        <div className="card-brand">
          <span className="brand-mark">
            <MessagesSquare />
          </span>
          Chat interna
        </div>
        <h1>Attiva la chat</h1>
        <div className="form-row">
          <label>
            Entri come
            <Help>
              L'account della chat è l'indirizzo di questa casella, con la stessa password della posta: Postamon la
              conosce già, non serve scriverla. I colleghi vedono il nome della casella. I messaggi e gli allegati
              restano dentro la rete aziendale.
            </Help>
          </label>
          <div className="chat-login-who">
            <strong>{account.name || account.email}</strong>
            <span>{account.email}</span>
          </div>
        </div>
        <div className="form-row">
          <label htmlFor="in-server">
            Server della chat
            <Help>Il computer dell'azienda che smista i messaggi. Cambialo solo se te lo chiedono.</Help>
          </label>
          <input id="in-server" className="input" value={server} onChange={(e) => setServer(e.target.value)} required />
        </div>
        {error && <div className="form-error">{error}</div>}
        <div className="card-actions" style={{ marginTop: error ? 14 : 6 }}>
          <button className="btn btn-primary" type="submit" disabled={busy}>
            {busy ? <LoaderCircle className="spin" /> : <LogIn />}
            {busy ? "Attivazione in corso…" : "Attiva la chat"}
          </button>
        </div>
      </form>
    </div>
  );
}

// ---- chat ----

function dayLabel(seconds: number): string {
  const d = new Date(seconds * 1000);
  const today = new Date();
  const start = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const diff = Math.round((start(today) - start(d)) / 86_400_000);
  if (diff === 0) return "Oggi";
  if (diff === 1) return "Ieri";
  return d.toLocaleDateString("it-IT", { weekday: "long", day: "numeric", month: "long" });
}

function timeLabel(seconds: number): string {
  return new Date(seconds * 1000).toLocaleTimeString("it-IT", { hour: "2-digit", minute: "2-digit" });
}

function Avatar({ user, size = 36 }: { user?: ChatUser; size?: number }) {
  const name = user?.displayName ?? "?";
  return (
    <span className="avatar chat-avatar" style={{ width: size, height: size, background: `hsl(${avatarHue(user?.username ?? name)} 42% 42%)` }}>
      {initials(name, user?.username ?? "")}
      {user?.online && <span className="online-dot" title="In linea" />}
    </span>
  );
}

interface MainProps {
  accountId: string;
  status: InternoStatus;
  me: number;
  dropEnabled: boolean;
  onToast: Props["onToast"];
  onLogout: () => void;
}

function ChatMain({ accountId, status, me, dropEnabled, onToast, onLogout }: MainProps) {
  const [users, setUsers] = useState<Map<number, ChatUser>>(new Map());
  const [conversations, setConversations] = useState<ChatConversation[]>([]);
  const [selected, setSelected] = useState<number | null>(null);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [hasOlder, setHasOlder] = useState(false);
  const [connected, setConnected] = useState(status.connected);
  const [query, setQuery] = useState("");
  const [newChat, setNewChat] = useState(false);
  const [text, setText] = useState("");
  const [files, setFiles] = useState<FileInfo[]>([]);
  const [sending, setSending] = useState(false);
  const [upload, setUpload] = useState<UploadProgress | null>(null);
  const [viewer, setViewer] = useState<ViewerState | null>(null);
  const viewerFile = useRef<ChatFile | null>(null);
  const selectedRef = useRef<number | null>(null);
  const thread = useRef<HTMLDivElement>(null);

  const loadUsers = useCallback(async () => {
    const list = await api.internoGet<ChatUser[]>(accountId, "/api/users");
    setUsers(new Map(list.map((u) => [u.id, u])));
  }, [accountId]);

  const loadConversations = useCallback(async () => {
    const list = await api.internoGet<ChatConversation[]>(accountId, "/api/conversations");
    setConversations(list);
    return list;
  }, [accountId]);

  const markRead = useCallback((conversationId: number, list: ChatMessage[]) => {
    const last = list[list.length - 1];
    if (!last) return;
    void api.internoPost(accountId, `/api/conversations/${conversationId}/read`, { messageId: last.id }).then(() =>
      setConversations((cs) => cs.map((c) => (c.id === conversationId ? { ...c, unread: 0 } : c))),
    );
  }, [accountId]);

  const openConversation = useCallback(
    async (id: number) => {
      selectedRef.current = id;
      setSelected(id);
      setMessages([]);
      try {
        const list = await api.internoGet<ChatMessage[]>(accountId, `/api/conversations/${id}/messages?limit=50`);
        if (selectedRef.current !== id) return;
        setMessages(list);
        setHasOlder(list.length === 50);
        markRead(id, list);
      } catch (e) {
        onToast(errorText(e), "error");
      }
    },
    [accountId, markRead, onToast],
  );

  useEffect(() => {
    void (async () => {
      try {
        await loadUsers();
        const list = await loadConversations();
        if (list.length > 0 && selectedRef.current === null) void openConversation(list[0].id);
      } catch (e) {
        onToast(errorText(e), "error");
      }
    })();
  }, [loadUsers, loadConversations, openConversation, onToast]);

  // Eventi in tempo reale dal server.
  useEffect(() => {
    const unlisten = [
      listen<ChatEventFor>("interno-event", ({ payload: { accountId: from, event: payload } }) => {
        // Gli eventi delle chat delle altre caselle non riguardano questo spazio.
        if (from !== accountId) return;
        if (payload.type === "message") {
          const m = payload.message;
          if (m.conversationId === selectedRef.current) {
            setMessages((list) => {
              if (list.some((x) => x.id === m.id)) return list;
              const next = [...list, m];
              if (document.hasFocus()) markRead(m.conversationId, next);
              return next;
            });
          }
          void loadConversations();
        } else if (payload.type === "conversation") {
          void loadConversations();
          void loadUsers();
        } else if (payload.type === "presence") {
          setUsers((map) => {
            const u = map.get(payload.userId);
            if (!u) return map;
            const next = new Map(map);
            next.set(u.id, { ...u, online: payload.online });
            return next;
          });
        }
      }),
      listen<ChatStatusFor>("interno-status", ({ payload }) => {
        if (payload.accountId !== accountId) return;
        setConnected(payload.connected);
        if (payload.connected) {
          void loadConversations();
          void loadUsers();
        }
      }),
      listen<UploadProgress>("interno-upload", ({ payload }) => payload.accountId === accountId && setUpload(payload)),
      listen<WebProgress>("craft-progress", ({ payload }) =>
        setViewer((v) => (v && !v.preview ? { ...v, progress: payload } : v)),
      ),
    ];
    return () => unlisten.forEach((u) => void u.then((f) => f()));
  }, [accountId, loadConversations, loadUsers, markRead]);

  // In fondo alla conversazione quando arrivano messaggi.
  useEffect(() => {
    thread.current?.scrollTo({ top: thread.current.scrollHeight });
  }, [messages.length, selected]);

  // File trascinati sulla finestra mentre la chat è aperta.
  useEffect(() => {
    if (!dropEnabled) return;
    let unlisten: (() => void) | undefined;
    void getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type === "drop" && selectedRef.current !== null) void addPaths(event.payload.paths);
      })
      .then((u) => (unlisten = u));
    return () => unlisten?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dropEnabled]);

  const addPaths = async (paths: string[]) => {
    if (!paths.length) return;
    try {
      const info = await api.filesInfo(paths);
      setFiles((prev) => [...prev, ...info.filter((f) => !prev.some((p) => p.path === f.path))]);
    } catch (e) {
      onToast(errorText(e), "error");
    }
  };

  const loadOlder = async () => {
    if (selected === null || messages.length === 0) return;
    const before = messages[0].id;
    const older = await api.internoGet<ChatMessage[]>(accountId, `/api/conversations/${selected}/messages?before=${before}&limit=50`);
    setHasOlder(older.length === 50);
    const box = thread.current;
    const height = box?.scrollHeight ?? 0;
    setMessages((list) => [...older, ...list]);
    requestAnimationFrame(() => box && box.scrollTo({ top: box.scrollHeight - height }));
  };

  const send = async () => {
    if (selected === null || sending) return;
    if (!text.trim() && files.length === 0) return;
    setSending(true);
    try {
      const message = await api.internoSend(accountId, selected, text, files.map((f) => f.path));
      setMessages((list) => (list.some((x) => x.id === message.id) ? list : [...list, message]));
      setText("");
      setFiles([]);
      void loadConversations();
    } catch (e) {
      onToast(errorText(e), "error");
    } finally {
      setSending(false);
      setUpload(null);
    }
  };

  const onComposerKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void send();
    }
  };

  const attach = async () => {
    const picked = await openDialog({ multiple: true, title: "Allega file" });
    if (picked) await addPaths(Array.isArray(picked) ? picked : [picked]);
  };

  // ---- allegati ricevuti ----

  const openFile = async (file: ChatFile) => {
    viewerFile.current = file;
    setViewer({ attachment: { index: 0, name: file.name, mime: file.mime, size: file.size }, preview: null, progress: null, error: "" });
    try {
      const preview = await api.internoFilePreview(accountId, file);
      if (viewerFile.current !== file) {
        if (preview) void api.previewClose(preview.token);
        return;
      }
      if (!preview) {
        setViewer(null);
        viewerFile.current = null;
        await api.internoFileOpen(accountId, file, false);
        return;
      }
      setViewer((v) => v && { ...v, preview });
    } catch (e) {
      if (viewerFile.current === file) setViewer((v) => v && { ...v, error: errorText(e) });
    }
  };

  const closeViewer = useCallback(() => {
    setViewer((v) => {
      if (v?.preview) void api.previewClose(v.preview.token);
      return null;
    });
    viewerFile.current = null;
  }, []);

  const saveFile = async (file: ChatFile) => {
    let defaultPath = file.name;
    try {
      defaultPath = await join(await downloadDir(), file.name);
    } catch {
      // cartella Download non trovata
    }
    const path = await save({ title: "Salva allegato", defaultPath });
    if (!path) return;
    try {
      await api.internoFileSave(accountId, file.id, path);
      onToast(`Salvato: ${file.name}`);
    } catch (e) {
      onToast(errorText(e), "error");
    }
  };

  // ---- presentazione ----

  const nameOf = (id: number) => users.get(id)?.displayName ?? "Collega";
  const titleOf = (c: ChatConversation) =>
    c.title ||
    c.members
      .filter((m) => m !== me)
      .map(nameOf)
      .join(", ") ||
    "Solo io";
  const otherOf = (c: ChatConversation) => (c.isGroup ? undefined : users.get(c.members.find((m) => m !== me) ?? -1));

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return q ? conversations.filter((c) => titleOf(c).toLowerCase().includes(q)) : conversations;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [conversations, query, users]);

  const current = conversations.find((c) => c.id === selected) ?? null;

  const logout = async () => {
    await api.internoLogout(accountId);
    onLogout();
  };

  return (
    <div className="chat">
      <section className="sheet chat-list">
        <div className="list-head">
          <div className="list-title">
            <h2>Chat interna</h2>
            <span className="sub">{status.user?.displayName}</span>
            <button className="btn btn-ghost icon-btn chat-logout" title="Esci dalla chat" aria-label="Esci dalla chat" onClick={() => void logout()}>
              <LogOut />
            </button>
          </div>
          <div className="chat-list-actions">
            <label className="search">
              <Search />
              <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Cerca una chat" />
            </label>
            <button className="btn btn-primary icon-btn" title="Nuova chat" aria-label="Nuova chat" onClick={() => setNewChat(true)}>
              <MessageSquarePlus />
            </button>
          </div>
        </div>
        {!connected && (
          <div className="banner">
            <WifiOff />
            <span className="grow">Server della chat non raggiungibile: riprovo da solo</span>
          </div>
        )}
        <div className="list">
          {filtered.length === 0 && (
            <div className="empty">
              <div>
                <MessagesSquare />
                <div>{conversations.length === 0 ? "Nessuna chat: iniziane una" : "Nessuna chat trovata"}</div>
              </div>
            </div>
          )}
          {filtered.map((c) => (
            <div key={c.id} className={`chat-row ${c.id === selected ? "selected" : ""} ${c.unread ? "unread" : ""}`} onClick={() => void openConversation(c.id)}>
              {c.isGroup ? (
                <span className="avatar chat-avatar group">
                  <Users />
                </span>
              ) : (
                <Avatar user={otherOf(c)} />
              )}
              <div className="chat-row-main">
                <div className="chat-row-top">
                  <span className="chat-row-title">{titleOf(c)}</span>
                  {c.lastMessage && <span className="row-date">{timeLabel(c.lastMessage.createdAt)}</span>}
                </div>
                <div className="chat-row-bottom">
                  <span className="chat-row-preview">
                    {c.lastMessage
                      ? `${c.lastMessage.senderId === me ? "Tu: " : c.isGroup ? `${nameOf(c.lastMessage.senderId).split(" ")[0]}: ` : ""}${
                          c.lastMessage.body || (c.lastMessage.files.length ? `Allegato: ${c.lastMessage.files[0].name}` : "")
                        }`
                      : "Nessun messaggio"}
                  </span>
                  {c.unread > 0 && <span className="chat-badge">{c.unread}</span>}
                </div>
              </div>
            </div>
          ))}
        </div>
      </section>

      <section className="sheet chat-thread">
        {current ? (
          <>
            <div className="chat-head">
              {current.isGroup ? (
                <span className="avatar chat-avatar group">
                  <Users />
                </span>
              ) : (
                <Avatar user={otherOf(current)} size={38} />
              )}
              <div className="chat-head-main">
                <div className="chat-head-title">{titleOf(current)}</div>
                <div className="chat-head-sub">
                  {current.isGroup
                    ? current.members.map(nameOf).join(", ")
                    : otherOf(current)?.online
                      ? "In linea"
                      : "Non in linea: riceverà i messaggi quando si collega"}
                </div>
              </div>
            </div>

            <div className="chat-messages selectable" ref={thread}>
              {hasOlder && (
                <div className="list-foot">
                  <button className="btn btn-small" onClick={() => void loadOlder()}>
                    Messaggi precedenti
                  </button>
                </div>
              )}
              {messages.map((m, i) => {
                const prev = messages[i - 1];
                const newDay = !prev || dayLabel(prev.createdAt) !== dayLabel(m.createdAt);
                const mine = m.senderId === me;
                const showName = current.isGroup && !mine && (!prev || prev.senderId !== m.senderId || newDay);
                return (
                  <div key={m.id}>
                    {newDay && <div className="chat-day">{dayLabel(m.createdAt)}</div>}
                    <div className={`bubble-row ${mine ? "mine" : ""}`}>
                      <div className="bubble">
                        {showName && <div className="bubble-name">{nameOf(m.senderId)}</div>}
                        {m.body && <div className="bubble-text">{m.body}</div>}
                        {m.files.map((f) => (
                          <div className="bubble-file" key={f.id}>
                            <button className="chip-open" onClick={() => void openFile(f)} title={`Apri ${f.name}`}>
                              <FileIcon className="kind" />
                              <span className="chip-text">
                                <span className="chip-name">{f.name}</span>
                                <span className="chip-size">{fileSize(f.size)}</span>
                              </span>
                            </button>
                            <button className="btn btn-ghost icon-btn" onClick={() => void saveFile(f)} title="Salva" aria-label={`Salva ${f.name}`}>
                              <Download />
                            </button>
                          </div>
                        ))}
                        <div className="bubble-time">{timeLabel(m.createdAt)}</div>
                      </div>
                    </div>
                  </div>
                );
              })}
            </div>

            {files.length > 0 && (
              <div className="compose-files">
                {files.map((f) => (
                  <div className="chip" key={f.path} title={f.path}>
                    <span className="chip-open">
                      <FileIcon className="kind" />
                      <span className="chip-text">
                        <span className="chip-name">{f.name}</span>
                        <span className="chip-size">{fileSize(f.size)}</span>
                      </span>
                    </span>
                    {!sending && (
                      <button className="btn btn-ghost icon-btn" aria-label={`Togli ${f.name}`} onClick={() => setFiles(files.filter((x) => x.path !== f.path))}>
                        <X />
                      </button>
                    )}
                  </div>
                ))}
              </div>
            )}
            {sending && upload && upload.total > 0 && (
              <div className="chat-upload">
                <LoaderCircle className="spin" />
                Invio di {upload.name} ({upload.index + 1} di {upload.count})
                <div className="bar">
                  <span style={{ width: `${Math.min(100, (upload.sent / upload.total) * 100)}%` }} />
                </div>
                <span className="bar-text">
                  {fileSize(upload.sent)} di {fileSize(upload.total)}
                </span>
              </div>
            )}
            <div className="chat-composer">
              <button className="btn btn-ghost icon-btn" onClick={() => void attach()} title="Allega file" aria-label="Allega file" disabled={sending}>
                <Paperclip />
              </button>
              <textarea
                value={text}
                onChange={(e) => setText(e.target.value)}
                onKeyDown={onComposerKey}
                placeholder="Scrivi un messaggio"
                rows={Math.min(6, Math.max(1, text.split("\n").length))}
                spellCheck
                lang="it"
              />
              <button className="btn btn-primary" onClick={() => void send()} disabled={sending || (!text.trim() && files.length === 0)}>
                {sending ? <LoaderCircle className="spin" /> : <SendHorizontal />}
                Invia
              </button>
            </div>
          </>
        ) : (
          <div className="empty">
            <div>
              <MessagesSquare />
              <div>Scegli una chat o iniziane una nuova</div>
            </div>
          </div>
        )}
      </section>

      {newChat && (
        <NewChatDialog
          users={[...users.values()].filter((u) => u.id !== me)}
          onClose={() => setNewChat(false)}
          onCreate={async (memberIds, title) => {
            try {
              const conv = await api.internoPost<ChatConversation>(accountId, "/api/conversations", { memberIds, title });
              setNewChat(false);
              await loadConversations();
              void openConversation(conv.id);
            } catch (e) {
              onToast(errorText(e), "error");
            }
          }}
        />
      )}

      {viewer && (
        <Viewer
          state={viewer}
          craftMissing={false}
          onClose={closeViewer}
          onSave={() => viewerFile.current && void saveFile(viewerFile.current)}
          onOpenNative={() =>
            viewerFile.current && void api.internoFileOpen(accountId, viewerFile.current, true).catch((e) => onToast(errorText(e), "error"))
          }
          onOpenSystem={() =>
            viewerFile.current && void api.internoFileOpen(accountId, viewerFile.current, false).catch((e) => onToast(errorText(e), "warn"))
          }
          onInstallCraft={() => {}}
        />
      )}
    </div>
  );
}

function NewChatDialog({
  users,
  onClose,
  onCreate,
}: {
  users: ChatUser[];
  onClose: () => void;
  onCreate: (memberIds: number[], title: string) => Promise<void>;
}) {
  const [query, setQuery] = useState("");
  const [picked, setPicked] = useState<number[]>([]);
  const [title, setTitle] = useState("");
  const [busy, setBusy] = useState(false);

  const list = useMemo(() => {
    const q = query.trim().toLowerCase();
    return users.filter((u) => !q || u.displayName.toLowerCase().includes(q) || u.username.includes(q));
  }, [users, query]);

  const toggle = (id: number) => setPicked((p) => (p.includes(id) ? p.filter((x) => x !== id) : [...p, id]));

  return (
    <div className="overlay" onKeyDown={(e) => e.key === "Escape" && onClose()}>
      <div className="dialog small new-chat">
        <div className="dialog-head">
          <h3>Nuova chat</h3>
          <button className="btn btn-ghost icon-btn" onClick={onClose} aria-label="Chiudi">
            <X />
          </button>
        </div>
        <div className="dialog-body">
          <label className="search">
            <Search />
            <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Cerca un collega" autoFocus />
          </label>
          <div className="people">
            {list.length === 0 && <div className="empty">Nessun collega trovato</div>}
            {list.map((u) => (
              <label key={u.id} className={`person ${picked.includes(u.id) ? "picked" : ""}`}>
                <input type="checkbox" checked={picked.includes(u.id)} onChange={() => toggle(u.id)} />
                <Avatar user={u} size={32} />
                <span className="person-name">{u.displayName}</span>
                <span className="person-user">{u.username}</span>
              </label>
            ))}
          </div>
          {picked.length > 1 && (
            <div className="form-row">
              <label htmlFor="group-title">Nome del gruppo</label>
              <input id="group-title" className="input" value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Negozio Arconate" />
            </div>
          )}
          <div className="card-actions">
            <button
              className="btn btn-primary"
              disabled={picked.length === 0 || busy}
              onClick={() => {
                setBusy(true);
                void onCreate(picked, title).finally(() => setBusy(false));
              }}
            >
              {busy ? <LoaderCircle className="spin" /> : <MessageSquarePlus />}
              {picked.length > 1 ? "Crea il gruppo" : "Apri la chat"}
            </button>
            <button className="btn" onClick={onClose}>
              Annulla
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

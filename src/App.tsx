import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { downloadDir, join } from "@tauri-apps/api/path";
import { open as openDialog, save } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { CircleCheck, CircleArrowUp, RefreshCw, RotateCw, Sparkles, SquarePen, TriangleAlert } from "lucide-react";
import {
  api,
  errorText,
  type Account,
  type Attachment,
  type ChatConversation,
  type ChatEventFor,
  type ChatStatusFor,
  type ClaudeAnswer,
  type ClaudeOpen,
  type ClaudeRequest,
  type ClaudeStatus,
  type Contact,
  type Company,
  type CompanyInput,
  type ContactInput,
  type CraftStatus,
  type Folder,
  type InstalledAddon,
  type Mailto,
  type MailUpdated,
  type MessagePage,
  type Opened,
  type SendReport,
  type SyncError,
  type UpdateInfo,
  type WebProgress,
} from "./api";
import { AccountForm } from "./components/AccountForm";
import { companyIndex } from "./book";
import { AddressBook } from "./components/AddressBook";
import {
  AddonFrame,
  AddonIcon,
  AddonPanel,
  addonEntries,
  type AddonDraft,
  type AddonEntry,
  type AddonMessageRef,
} from "./components/AddonFrame";
import { AddonsDialog } from "./components/AddonsDialog";
import { ArtCraftSetup } from "./components/ArtCraftSetup";
import { ChatView } from "./components/ChatView";
import { ClaudePanel } from "./components/ClaudePanel";
import { ClaudeRequestDialog } from "./components/ClaudeRequestDialog";
import { Compose, type ComposeInit } from "./components/Compose";
import { Help } from "./components/Help";
import { MessageList } from "./components/MessageList";
import { Rail } from "./components/Rail";
import { Reader } from "./components/Reader";
import { SignaturesDialog } from "./components/SignaturesDialog";
import { Sidebar } from "./components/Sidebar";
import { Viewer, type ViewerState } from "./components/Viewer";

const PAGE = 200;
const CLAUDE_OPEN_KEY = "postamon.claude-open";

function claudeWasOpen(): boolean {
  try {
    return localStorage.getItem(CLAUDE_OPEN_KEY) === "1";
  } catch {
    return false;
  }
}

type Current = { accountId: string; folder: string };
type ToastKind = "info" | "warn" | "error";
/** Cosa mostra lo spazio di lavoro: la posta, la chat, la rubrica o la vista di un add-on. */
type View = "mail" | "chat" | "contacts" | `addon:${string}`;

export default function App() {
  const [accounts, setAccounts] = useState<Account[] | null>(null);
  const [folders, setFolders] = useState<Record<string, Folder[]>>({});
  const [current, setCurrent] = useState<Current | null>(null);
  const [page, setPage] = useState<MessagePage | null>(null);
  const [selectedUid, setSelectedUid] = useState<number | null>(null);
  const [opened, setOpened] = useState<Opened | null>(null);
  const [opening, setOpening] = useState(false);
  const [openError, setOpenError] = useState("");
  const [syncing, setSyncing] = useState<Record<string, boolean>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [lastSync, setLastSync] = useState<Date | null>(null);
  const [loadingOlder, setLoadingOlder] = useState(false);
  const [compose, setCompose] = useState<ComposeInit | null>(null);
  const [addingAccount, setAddingAccount] = useState(false);
  const [signatureFor, setSignatureFor] = useState<Account | null>(null);
  const [toast, setToast] = useState<{ text: string; kind: ToastKind } | null>(null);
  const [craftStatus, setCraftStatus] = useState<CraftStatus | null>(null);
  const [craftSetup, setCraftSetup] = useState(false);
  const [view, setView] = useState<View>("mail");
  // Chat da leggere per casella (ogni casella è uno spazio di lavoro con la sua chat).
  const [chatUnread, setChatUnread] = useState<Record<string, number>>({});
  // Rubrica di ogni casella (persone e schede delle aziende) e contatto aperto nella vista Rubrica.
  const [books, setBooks] = useState<Record<string, Contact[]>>({});
  const [companies, setCompanies] = useState<Record<string, Company[]>>({});
  const [contactSel, setContactSel] = useState<number | null>(null);
  // Dove si era rimasti in ogni spazio di lavoro, per tornarci.
  const places = useRef<Record<string, { folder: string; view: View }>>({});
  const [addons, setAddons] = useState<InstalledAddon[]>([]);
  const [addonsOpen, setAddonsOpen] = useState(false);
  // Add-on aperto dal pulsante sul messaggio, legato a quel messaggio.
  const [addonPanel, setAddonPanel] = useState<{ entry: AddonEntry; message: AddonMessageRef } | null>(null);
  // Cresce a ogni "Ricarica" degli add-on in sviluppo: le loro pagine ripartono.
  const [addonReload, setAddonReload] = useState(0);
  const [viewer, setViewer] = useState<ViewerState | null>(null);
  // Nuova versione di Postamon già scaricata, in attesa del riavvio.
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [updating, setUpdating] = useState(false);
  // Messaggio a cui appartiene l'allegato aperto nell'anteprima.
  const viewerSource = useRef<{ accountId: string; folder: string; uid: number } | null>(null);
  // Pannello Claude: resta montato dopo la prima apertura, così una risposta
  // in corso non si perde chiudendolo.
  const [claudeOpen, setClaudeOpen] = useState(claudeWasOpen);
  const [claudeMounted, setClaudeMounted] = useState(claudeOpen);
  const [claudeStatus, setClaudeStatus] = useState<ClaudeStatus | null>(null);
  // Azioni chieste da Claude che aspettano la conferma.
  const [claudeRequests, setClaudeRequests] = useState<ClaudeRequest[]>([]);
  const [answering, setAnswering] = useState(false);

  // Le callback degli eventi del motore leggono lo stato attuale da qui.
  const currentRef = useRef<Current | null>(null);
  const limitRef = useRef(PAGE);
  const selectedRef = useRef<number | null>(null);
  const toastTimer = useRef<number | undefined>(undefined);

  const showToast = useCallback((text: string, kind: ToastKind = "info") => {
    setToast({ text, kind });
    window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(null), kind === "info" ? 3500 : 7000);
  }, []);

  const loadFolders = useCallback(async (accountId: string) => {
    try {
      const list = await api.folders(accountId);
      setFolders((prev) => ({ ...prev, [accountId]: list }));
    } catch {
      // casella appena tolta
    }
  }, []);

  const loadBook = useCallback(async (accountId: string) => {
    try {
      const [list, cards] = await Promise.all([api.addressBook(accountId), api.companies(accountId)]);
      setBooks((prev) => ({ ...prev, [accountId]: list }));
      setCompanies((prev) => ({ ...prev, [accountId]: cards }));
    } catch {
      // casella appena tolta
    }
  }, []);

  const saveContact = useCallback(
    async (accountId: string, input: ContactInput) => {
      const { contact: saved, companyShared } = await api.saveContact(accountId, input);
      setBooks((prev) => ({
        ...prev,
        [accountId]: [...(prev[accountId] ?? []).filter((c) => c.id !== saved.id), saved],
      }));
      // L'azienda può essere passata ai colleghi o la sua scheda aver preso il
      // nome nuovo: si rilegge la rubrica.
      void loadBook(accountId);
      if (companyShared > 0) {
        const domain = saved.address.slice(saved.address.lastIndexOf("@"));
        showToast(
          companyShared === 1
            ? `«${saved.company}» anche per un altro contatto di ${domain}`
            : `«${saved.company}» anche per altri ${companyShared} contatti di ${domain}`,
        );
      }
      return saved;
    },
    [loadBook, showToast],
  );

  const saveCompany = useCallback(
    async (accountId: string, input: CompanyInput) => {
      const { company, contactsRenamed } = await api.saveCompany(accountId, input);
      await loadBook(accountId);
      if (contactsRenamed > 0) {
        showToast(
          contactsRenamed === 1
            ? `Nome aggiornato anche per la persona di «${company.name}»`
            : `Nome aggiornato anche per le ${contactsRenamed} persone di «${company.name}»`,
        );
      }
      return company;
    },
    [loadBook, showToast],
  );

  const deleteContact = useCallback(
    async (accountId: string, contact: Contact) => {
      try {
        await api.deleteContact(accountId, contact.id);
        setBooks((prev) => ({ ...prev, [accountId]: (prev[accountId] ?? []).filter((c) => c.id !== contact.id) }));
        setContactSel((id) => (id === contact.id ? null : id));
      } catch (e) {
        showToast(errorText(e), "error");
      }
    },
    [showToast],
  );

  // Mittente → contatto, per la rubrica della casella aperta.
  const bookIndex = useMemo(() => {
    const map = new Map<string, Contact>();
    for (const c of books[current?.accountId ?? ""] ?? []) map.set(c.address, c);
    return map;
  }, [books, current?.accountId]);
  const contactOf = useCallback((address: string) => bookIndex.get(address.toLowerCase()), [bookIndex]);
  // Aziende già in rubrica, proposte nel campo "Azienda" della finestrella.
  const companyNames = useMemo(() => {
    const id = current?.accountId ?? "";
    const { companies: all } = companyIndex(books[id] ?? [], companies[id] ?? []);
    return [...all.values()].map((c) => c.name).sort((a, b) => a.localeCompare(b, "it"));
  }, [books, companies, current?.accountId]);

  const loadMessages = useCallback(async (accountId: string, folder: string) => {
    const result = await api.messages(accountId, folder, limitRef.current);
    const cur = currentRef.current;
    if (cur && cur.accountId === accountId && cur.folder === folder) setPage(result);
  }, []);

  const clearError = (accountId: string) =>
    setErrors((e) => {
      if (!(accountId in e)) return e;
      const next = { ...e };
      delete next[accountId];
      return next;
    });

  const syncFolder = useCallback(
    async (accountId: string, folder: string, refreshFolders: boolean) => {
      setSyncing((s) => ({ ...s, [accountId]: true }));
      try {
        await api.sync(accountId, folder, refreshFolders);
        clearError(accountId);
        setLastSync(new Date());
      } catch (e) {
        setErrors((er) => ({ ...er, [accountId]: errorText(e) }));
      } finally {
        setSyncing((s) => ({ ...s, [accountId]: false }));
      }
      await loadFolders(accountId);
      const cur = currentRef.current;
      if (cur && cur.accountId === accountId && cur.folder === folder) await loadMessages(accountId, folder);
    },
    [loadFolders, loadMessages],
  );

  const selectFolder = useCallback(
    (accountId: string, folder: string, sync = true) => {
      const next = { accountId, folder };
      currentRef.current = next;
      limitRef.current = PAGE;
      selectedRef.current = null;
      setCurrent(next);
      setPage(null);
      setSelectedUid(null);
      setOpened(null);
      setOpenError("");
      void loadMessages(accountId, folder);
      if (sync) void syncFolder(accountId, folder, false);
    },
    [loadMessages, syncFolder],
  );

  // Avvio: caselle, cartelle dall'archivio, poi aggiornamento dal server.
  useEffect(() => {
    void (async () => {
      const list = await api.accounts();
      setAccounts(list);
      list.forEach((a) => void loadBook(a.id));
      await Promise.all(list.map((a) => loadFolders(a.id)));
      if (list.length > 0) {
        selectFolder(list[0].id, "INBOX", false);
        for (const a of list) void syncFolder(a.id, "INBOX", true);
      }
      // Se sul PC mancano le app ArtCraft si propone di installarle.
      const status = await api.craftStatus();
      setCraftStatus(status);
      if (list.length > 0 && status.missing.length > 0) setCraftSetup(true);
    })();
  }, [loadFolders, loadBook, selectFolder, syncFolder]);

  // Messaggi da leggere nelle chat delle caselle, per i numeri sulle icone.
  useEffect(() => {
    if (!accounts) return;
    const refreshOne = async (accountId: string) => {
      try {
        const status = await api.internoStatus(accountId);
        const list = status.configured
          ? await api.internoGet<ChatConversation[]>(accountId, "/api/conversations")
          : [];
        setChatUnread((u) => ({ ...u, [accountId]: list.reduce((n, c) => n + c.unread, 0) }));
      } catch {
        // server della chat non raggiungibile: si lascia il numero com'era
      }
    };
    const refreshAll = () => accounts.forEach((a) => void refreshOne(a.id));
    refreshAll();
    const unlisten = [
      listen<ChatEventFor>("interno-event", ({ payload }) => void refreshOne(payload.accountId)),
      listen<ChatStatusFor>("interno-status", ({ payload }) => void refreshOne(payload.accountId)),
    ];
    const timer = window.setInterval(refreshAll, 30_000);
    return () => {
      window.clearInterval(timer);
      unlisten.forEach((u) => void u.then((f) => f()));
    };
  }, [accounts]);

  // Si annota dove si è nello spazio di lavoro aperto.
  useEffect(() => {
    if (current) places.current[current.accountId] = { folder: current.folder, view };
  }, [current, view]);

  const switchWorkspace = useCallback(
    (accountId: string) => {
      if (currentRef.current?.accountId === accountId) return;
      const place = places.current[accountId] ?? { folder: "INBOX", view: "mail" as View };
      setView(place.view);
      selectFolder(accountId, place.folder);
    },
    [selectFolder],
  );

  const reloadAddons = useCallback(async (frames = false) => {
    try {
      setAddons(await api.addonsInstalled());
    } catch {
      // si tiene l'elenco di prima
    }
    if (frames) setAddonReload((n) => n + 1);
  }, []);

  useEffect(() => {
    void reloadAddons();
  }, [reloadAddons]);

  const closeAddonPanel = useCallback(() => setAddonPanel(null), []);
  const closeAddons = useCallback(() => setAddonsOpen(false), []);

  // Aggiornamenti: il motore li scarica in sottofondo e avvisa quando sono pronti.
  useEffect(() => {
    void api.updateStatus().then((s) => setUpdate(s.ready));
    const unlisten = listen<UpdateInfo>("update-ready", ({ payload }) => setUpdate(payload));
    return () => void unlisten.then((f) => f());
  }, []);

  const installUpdate = useCallback(async () => {
    setUpdating(true);
    try {
      // Se va a buon fine Postamon si chiude qui e l'installer lo riapre.
      await api.updateInstall();
    } catch (e) {
      setUpdating(false);
      setUpdate(null);
      showToast(errorText(e), "error");
    }
  }, [showToast]);

  // Avanzamento del primo download dell'app che mostra l'anteprima.
  useEffect(() => {
    const unlisten = listen<WebProgress>("craft-progress", ({ payload }) =>
      setViewer((v) => (v && !v.preview ? { ...v, progress: payload } : v)),
    );
    return () => void unlisten.then((f) => f());
  }, []);

  // Segnali del controllo periodico in sottofondo.
  useEffect(() => {
    const unlisten = [
      listen<MailUpdated>("mail-updated", ({ payload }) => {
        void loadFolders(payload.accountId);
        const cur = currentRef.current;
        if (cur && cur.accountId === payload.accountId && cur.folder === payload.folder) {
          void loadMessages(cur.accountId, cur.folder);
        }
      }),
      listen<SyncError>("sync-error", ({ payload }) =>
        setErrors((e) => ({ ...e, [payload.accountId]: payload.message })),
      ),
      listen<MailUpdated>("sync-ok", ({ payload }) => {
        clearError(payload.accountId);
        setLastSync(new Date());
      }),
    ];
    return () => unlisten.forEach((u) => void u.then((f) => f()));
  }, [loadFolders, loadMessages]);

  // Aggiorna lo spazio di lavoro aperto (le altre caselle le controlla il
  // motore in sottofondo ogni minuto).
  const refresh = useCallback(() => {
    const cur = currentRef.current;
    if (cur) void syncFolder(cur.accountId, cur.folder, true);
  }, [syncFolder]);

  const openMessage = useCallback(
    async (uid: number, allowRemote = false) => {
      const cur = currentRef.current;
      if (!cur) return;
      selectedRef.current = uid;
      setSelectedUid(uid);
      setOpening(true);
      setOpenError("");
      if (!allowRemote) setOpened(null);
      try {
        const result = await api.open(cur.accountId, cur.folder, uid, allowRemote);
        if (selectedRef.current !== uid || currentRef.current !== cur) return;
        setOpened(result);
        setPage((p) => p && { ...p, items: p.items.map((m) => (m.uid === uid ? { ...m, seen: true } : m)) });
        void loadFolders(cur.accountId);
      } catch (e) {
        if (selectedRef.current === uid) setOpenError(errorText(e));
      } finally {
        if (selectedRef.current === uid) setOpening(false);
      }
    },
    [loadFolders],
  );

  const newMessage = useCallback(
    (fields: Partial<Mailto> = {}) => {
      if (!accounts?.length) return;
      const account = accounts.find((a) => a.id === currentRef.current?.accountId) ?? accounts[0];
      setCompose({
        accountId: account.id,
        to: fields.to ?? "",
        cc: fields.cc ?? "",
        bcc: fields.bcc ?? "",
        subject: fields.subject ?? "",
        body: fields.body ?? "",
        quote: "",
        signatureId: account.signatureNew,
        inReplyTo: null,
        references: null,
        reply: null,
        forward: null,
        forwardAttachments: [],
      });
    },
    [accounts],
  );

  // Link email aperti con Postamon da Windows (browser, documenti, altri
  // programmi): diventano un nuovo messaggio nello spazio di lavoro aperto.
  // Con un messaggio già in scrittura il link aspetta che lo si chiuda.
  const [mailtoSignal, setMailtoSignal] = useState(0);
  const composeOpen = compose !== null;
  const composeOpenRef = useRef(composeOpen);
  composeOpenRef.current = composeOpen;
  useEffect(() => {
    const unlisten = listen("mailto", () => {
      if (composeOpenRef.current) showToast("Il link email si apre quando chiudi il messaggio che stai scrivendo");
      setMailtoSignal((n) => n + 1);
    });
    return () => void unlisten.then((f) => f());
  }, [showToast]);
  useEffect(() => {
    if (!accounts?.length || composeOpen) return;
    void api.mailtoTake().then((link) => link && newMessage(link));
  }, [accounts, composeOpen, mailtoSignal, newMessage]);

  // ---- Claude ----

  const refreshClaude = useCallback(async () => {
    try {
      const status = await api.claudeStatus();
      setClaudeStatus(status);
      return status;
    } catch (e) {
      showToast(errorText(e), "error");
      return null;
    }
  }, [showToast]);

  const toggleClaude = () => {
    const open = !claudeOpen;
    setClaudeOpen(open);
    if (open) setClaudeMounted(true);
    try {
      localStorage.setItem(CLAUDE_OPEN_KEY, open ? "1" : "0");
    } catch {
      // si riapre chiuso
    }
  };

  useEffect(() => {
    if (claudeOpen && !claudeStatus) void refreshClaude();
  }, [claudeOpen, claudeStatus, refreshClaude]);

  const loadClaudeRequests = useCallback(() => {
    void api
      .claudeRequests()
      .then(setClaudeRequests)
      .catch(() => {});
  }, []);

  useEffect(() => {
    loadClaudeRequests();
    const unlisten = listen("claude-request", loadClaudeRequests);
    return () => void unlisten.then((f) => f());
  }, [loadClaudeRequests]);

  const answerClaude = async (request: ClaudeRequest, answer: ClaudeAnswer) => {
    setAnswering(true);
    try {
      const outcome = await api.claudeAnswer(request.id, answer);
      if (answer === "edit" && request.kind === "send") setCompose(request.detail.draft);
      else if (answer === "accept") showToast(outcome);
    } catch (e) {
      showToast(errorText(e), "error");
    } finally {
      setClaudeRequests((list) => list.filter((r) => r.id !== request.id));
      setAnswering(false);
      loadClaudeRequests();
    }
  };

  // Claude sa cosa sta guardando l'utente («riassumi questo messaggio»).
  useEffect(() => {
    void api
      .claudeContext({
        accountId: current?.accountId ?? null,
        folder: view === "mail" ? (current?.folder ?? null) : null,
        uid: view === "mail" ? selectedUid : null,
        composeOpen,
      })
      .catch(() => {});
  }, [current, view, selectedUid, composeOpen]);

  // Messaggio preparato da Claude: si apre nella finestra di scrittura.
  useEffect(() => {
    const unlisten = listen<ComposeInit>("claude-compose", ({ payload }) => {
      if (composeOpenRef.current) showToast("Claude ha preparato un messaggio, ma ne stai già scrivendo uno", "warn");
      else setCompose(payload);
    });
    return () => void unlisten.then((f) => f());
  }, [showToast]);

  // Claude mostra una cartella, un messaggio o la pagina di un add-on.
  const addonsRef = useRef(addons);
  addonsRef.current = addons;
  useEffect(() => {
    const unlisten = [
      listen<ClaudeOpen>("claude-open", ({ payload }) => {
        if (payload.addon) {
          const entry = addonEntries(addonsRef.current, "sidebar-view").find((e) => e.addon.id === payload.addon);
          if (entry) setView(`addon:${entry.key}`);
          return;
        }
        if (!payload.accountId) return;
        setView("mail");
        selectFolder(payload.accountId, payload.folder ?? "INBOX");
        if (payload.uid != null) void openMessage(payload.uid);
      }),
      listen("addons-changed", () => void reloadAddons()),
    ];
    return () => unlisten.forEach((u) => void u.then((f) => f()));
  }, [selectFolder, openMessage, reloadAddons]);

  // Scorciatoie: Ctrl+N nuovo messaggio, F5 aggiorna.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (compose || addingAccount || signatureFor || addonsOpen || addonPanel || claudeRequests.length > 0) return;
      if (e.ctrlKey && e.key.toLowerCase() === "n") {
        e.preventDefault();
        newMessage();
      } else if (e.key === "F5") {
        e.preventDefault();
        refresh();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [compose, addingAccount, signatureFor, addonsOpen, addonPanel, claudeRequests, newMessage, refresh]);

  // ---- richieste degli add-on ----
  // Funzioni normali, non memorizzate: il riquadro dell'add-on legge sempre le
  // ultime, quindi vedono il messaggio in scrittura di adesso.

  const composeFromAddon = (draft: AddonDraft) => {
    if (compose) throw new Error("C'è già un messaggio in scrittura: chiudilo prima");
    if (!accounts?.length) throw new Error("Nessuna casella configurata");
    const account = accounts.find((a) => a.id === currentRef.current?.accountId) ?? accounts[0];
    setCompose({
      accountId: account.id,
      to: draft.to,
      cc: draft.cc,
      bcc: "",
      subject: draft.subject,
      body: draft.body,
      quote: "",
      signatureId: account.signatureNew,
      inReplyTo: null,
      references: null,
      reply: null,
      forward: null,
      forwardAttachments: [],
    });
  };

  const replyFromAddon = async (message: AddonMessageRef, body: string, all: boolean) => {
    if (compose) throw new Error("C'è già un messaggio in scrittura: chiudilo prima");
    const data = await api.composeFrom(message.accountId, message.folder, message.uid, all ? "replyAll" : "reply");
    // Il testo dell'add-on va sopra la firma e la citazione.
    setCompose({ ...data, bcc: "", body: body.trimEnd() });
  };

  const openAddonButton = (entry: AddonEntry) => {
    const cur = currentRef.current;
    if (!cur || selectedUid === null) return;
    setAddonPanel({ entry, message: { accountId: cur.accountId, folder: cur.folder, uid: selectedUid } });
  };

  const selected = page?.items.find((m) => m.uid === selectedUid) ?? null;

  const reply = async (mode: "reply" | "replyAll" | "forward") => {
    const cur = currentRef.current;
    if (!cur || selectedUid === null) return;
    try {
      const data = await api.composeFrom(cur.accountId, cur.folder, selectedUid, mode);
      setCompose({ ...data, bcc: "" });
    } catch (e) {
      showToast(errorText(e), "error");
    }
  };

  const toggleSeen = async () => {
    const cur = currentRef.current;
    if (!cur || !selected) return;
    const seen = !selected.seen;
    try {
      await api.setSeen(cur.accountId, cur.folder, selected.uid, seen);
      setPage((p) => p && { ...p, items: p.items.map((m) => (m.uid === selected.uid ? { ...m, seen } : m)) });
      void loadFolders(cur.accountId);
    } catch (e) {
      showToast(errorText(e), "error");
    }
  };

  const toggleFlag = async () => {
    const cur = currentRef.current;
    if (!cur || !selected) return;
    const flagged = !selected.flagged;
    try {
      await api.setFlagged(cur.accountId, cur.folder, selected.uid, flagged);
      setPage((p) => p && { ...p, items: p.items.map((m) => (m.uid === selected.uid ? { ...m, flagged } : m)) });
    } catch (e) {
      showToast(errorText(e), "error");
    }
  };

  const deleteMessage = async (uid: number) => {
    const cur = currentRef.current;
    if (!cur || !page) return;
    const idx = page.items.findIndex((m) => m.uid === uid);
    const next = page.items[idx + 1] ?? page.items[idx - 1] ?? null;
    try {
      await api.remove(cur.accountId, cur.folder, uid);
      setPage((p) => p && { ...p, items: p.items.filter((m) => m.uid !== uid), cached: p.cached - 1, total: Math.max(0, p.total - 1) });
      void loadFolders(cur.accountId);
      if (next) void openMessage(next.uid);
      else {
        selectedRef.current = null;
        setSelectedUid(null);
        setOpened(null);
      }
    } catch (e) {
      showToast(errorText(e), "error");
    }
  };

  const loadOlder = async () => {
    const cur = currentRef.current;
    if (!cur || !page) return;
    setLoadingOlder(true);
    try {
      if (page.cached <= page.items.length) await api.loadOlder(cur.accountId, cur.folder);
      limitRef.current += PAGE;
      await loadMessages(cur.accountId, cur.folder);
    } catch (e) {
      showToast(errorText(e), "error");
    } finally {
      setLoadingOlder(false);
    }
  };

  const showImages = async (always: boolean) => {
    if (selectedUid === null || !opened) return;
    if (always && opened.fromAddr) {
      try {
        await api.trustSender(opened.fromAddr);
      } catch (e) {
        showToast(errorText(e), "error");
      }
    }
    await openMessage(selectedUid, true);
  };

  const openAttachment = async (a: Attachment) => {
    const cur = currentRef.current;
    if (!cur || selectedUid === null) return;
    try {
      await api.openAttachment(cur.accountId, cur.folder, selectedUid, a.index);
    } catch (e) {
      showToast(errorText(e), "warn");
    }
  };

  // Allegato aperto dentro Postamon con l'app ArtCraft adatta; i tipi che
  // nessuna app ArtCraft apre vanno al programma predefinito di Windows.
  const previewAttachment = async (a: Attachment) => {
    const cur = currentRef.current;
    if (!cur || selectedUid === null) return;
    const source = { accountId: cur.accountId, folder: cur.folder, uid: selectedUid };
    viewerSource.current = source;
    setViewer({ attachment: a, preview: null, progress: null, error: "" });
    try {
      const preview = await api.preview(source.accountId, source.folder, source.uid, a.index);
      if (viewerSource.current !== source) {
        if (preview) void api.previewClose(preview.token);
        return;
      }
      if (!preview) {
        viewerSource.current = null;
        setViewer(null);
        await openAttachment(a);
        return;
      }
      setViewer((v) => v && { ...v, preview });
    } catch (e) {
      if (viewerSource.current === source) setViewer((v) => v && { ...v, error: errorText(e) });
    }
  };

  const closeViewer = useCallback(() => {
    setViewer((v) => {
      if (v?.preview) void api.previewClose(v.preview.token);
      return null;
    });
    viewerSource.current = null;
  }, []);

  const openViewerNative = async () => {
    const src = viewerSource.current;
    if (!src || !viewer) return;
    try {
      await api.openNative(src.accountId, src.folder, src.uid, viewer.attachment.index);
    } catch (e) {
      showToast(errorText(e), "error");
    }
  };

  const saveAttachment = async (a: Attachment) => {
    const cur = currentRef.current;
    if (!cur || selectedUid === null) return;
    let defaultPath = a.name;
    try {
      defaultPath = await join(await downloadDir(), a.name);
    } catch {
      // cartella Download non trovata: si propone solo il nome
    }
    const path = await save({ title: "Salva allegato", defaultPath });
    if (!path) return;
    try {
      await api.saveAttachment(cur.accountId, cur.folder, selectedUid, a.index, path);
      showToast(`Salvato: ${a.name}`);
    } catch (e) {
      showToast(errorText(e), "error");
    }
  };

  const saveAllAttachments = async () => {
    const cur = currentRef.current;
    if (!cur || selectedUid === null) return;
    const uid = selectedUid;
    let defaultPath: string | undefined;
    try {
      defaultPath = await downloadDir();
    } catch {
      // cartella Download non trovata: decide la finestra di Windows
    }
    const dir = await openDialog({ title: "Scarica tutti gli allegati in…", directory: true, defaultPath });
    if (typeof dir !== "string") return;
    try {
      const count = await api.saveAllAttachments(cur.accountId, cur.folder, uid, dir);
      const where = dir.split(/[\\/]/).filter(Boolean).pop() ?? dir;
      showToast(`Salvati ${count} allegati in ${where}`);
    } catch (e) {
      showToast(errorText(e), "error");
    }
  };

  const onSent = (report: SendReport) => {
    setCompose(null);
    if (report.warning) showToast(report.warning, "warn");
    else showToast("Messaggio inviato");
    const cur = currentRef.current;
    if (cur) void loadFolders(cur.accountId);
  };

  const onAccountAdded = async (account: Account) => {
    setAddingAccount(false);
    const list = await api.accounts();
    setAccounts(list);
    void loadBook(account.id);
    await loadFolders(account.id);
    setView("mail");
    selectFolder(account.id, "INBOX", false);
    void syncFolder(account.id, "INBOX", true);
  };

  const removeAccount = async (account: Account) => {
    try {
      await api.removeAccount(account.id);
      const list = await api.accounts();
      setAccounts(list);
      setFolders((f) => {
        const next = { ...f };
        delete next[account.id];
        return next;
      });
      setBooks((b) => {
        const next = { ...b };
        delete next[account.id];
        return next;
      });
      setCompanies((c) => {
        const next = { ...c };
        delete next[account.id];
        return next;
      });
      delete places.current[account.id];
      if (currentRef.current?.accountId === account.id) {
        if (list.length) {
          setView("mail");
          selectFolder(list[0].id, "INBOX");
        } else {
          currentRef.current = null;
          setCurrent(null);
        }
      }
    } catch (e) {
      showToast(errorText(e), "error");
    }
  };

  if (accounts === null) return null;

  if (accounts.length === 0) {
    return (
      <div className="setup">
        <AccountForm first onAdded={(a) => void onAccountAdded(a)} />
      </div>
    );
  }

  const currentAccount = accounts.find((a) => a.id === current?.accountId) ?? accounts[0];
  const workspaceFolders = folders[currentAccount.id] ?? [];
  const currentFolder = workspaceFolders.find((f) => f.name === current?.folder);
  const workspaceSyncing = syncing[currentAccount.id] ?? false;
  const unseen = currentFolder?.unseen ?? 0;
  const showRecipient = currentFolder?.role === "sent" || currentFolder?.role === "drafts";
  const addonButtons = addonEntries(addons, "message-button");
  const addonViews = addonEntries(addons, "sidebar-view");
  // Vista di add-on aperta; se nel frattempo è stato disattivato si torna alla posta.
  const addonView = view.startsWith("addon:") ? addonViews.find((e) => `addon:${e.key}` === view) : undefined;
  // Numero sull'icona di ogni casella: posta in arrivo da leggere più chat.
  const badges = Object.fromEntries(
    accounts.map((a) => [
      a.id,
      (folders[a.id]?.find((f) => f.role === "inbox")?.unseen ?? 0) + (chatUnread[a.id] ?? 0),
    ]),
  );

  return (
    <div className="app">
      <Rail
        accounts={accounts}
        current={currentAccount.id}
        badges={badges}
        onSelect={switchWorkspace}
        onAdd={() => setAddingAccount(true)}
        onAddons={() => setAddonsOpen(true)}
      />
      <header className="topbar">
        <button className="btn btn-primary" onClick={() => newMessage()}>
          <SquarePen />
          Nuovo messaggio
        </button>
        <button className="btn" onClick={refresh} disabled={workspaceSyncing}>
          <RefreshCw className={workspaceSyncing ? "spin" : ""} />
          Aggiorna
        </button>
        <div className="topbar-status">
          {workspaceSyncing
            ? "Aggiornamento in corso…"
            : lastSync
              ? `Aggiornato alle ${lastSync.toLocaleTimeString("it-IT", { hour: "2-digit", minute: "2-digit" })}`
              : ""}
        </div>
        {update && (
          <div className="update-ready" role="status">
            <CircleArrowUp />
            <span>Postamon {update.version} è pronto</span>
            {update.notes && <Help align="left">{update.notes}</Help>}
            <button className="btn btn-primary btn-small" onClick={() => void installUpdate()} disabled={updating}>
              <RotateCw className={updating ? "spin" : ""} />
              {updating ? "Aggiornamento…" : "Riavvia e aggiorna"}
            </button>
          </div>
        )}
        <button
          className={`btn claude-toggle ${claudeOpen ? "active" : ""}`}
          onClick={toggleClaude}
          aria-pressed={claudeOpen}
        >
          <Sparkles />
          Claude
        </button>
      </header>

      <div className={`workspace ${claudeOpen ? "with-claude" : ""}`}>
        <Sidebar
          account={currentAccount}
          folders={workspaceFolders}
          folder={current?.folder ?? null}
          syncing={workspaceSyncing}
          error={errors[currentAccount.id]}
          onSelect={(folder) => {
            setView("mail");
            selectFolder(currentAccount.id, folder);
          }}
          onEditSignature={setSignatureFor}
          onRemoveAccount={(a) => void removeAccount(a)}
          onMailtoSettings={() => void api.mailtoSettings().catch((e) => showToast(errorText(e), "error"))}
          chatActive={view === "chat"}
          chatUnread={chatUnread[currentAccount.id] ?? 0}
          onOpenChat={() => setView("chat")}
          contactsActive={view === "contacts"}
          onOpenContacts={() => setView("contacts")}
          addonViews={addonViews}
          activeAddon={addonView?.key ?? null}
          onOpenAddon={(key) => setView(`addon:${key}`)}
        />
        {view === "contacts" ? (
          <AddressBook
            account={currentAccount}
            contacts={books[currentAccount.id] ?? null}
            companies={companies[currentAccount.id] ?? []}
            selectedId={contactSel}
            onSelect={setContactSel}
            onSave={(input) => saveContact(currentAccount.id, input)}
            suggestCompany={(address) => api.companyFor(currentAccount.id, address)}
            onDelete={(c) => deleteContact(currentAccount.id, c)}
            onSaveCompany={(input) => saveCompany(currentAccount.id, input)}
            onWrite={(to) => newMessage({ to })}
          />
        ) : view === "chat" ? (
          <ChatView
            key={currentAccount.id}
            account={currentAccount}
            dropEnabled={!compose && !viewer && !addonsOpen && !addonPanel}
            onToast={showToast}
          />
        ) : addonView ? (
          <section className="sheet addon-view">
            <div className="addon-view-head">
              <AddonIcon addon={addonView.addon} size={18} />
              <h2>{addonView.hook.title}</h2>
              <span className="addon-by">
                {addonView.addon.manifest?.name !== addonView.hook.title && addonView.addon.manifest?.name}
              </span>
              {addonView.addon.dev && (
                <button className="btn btn-small" onClick={() => void reloadAddons(true)}>
                  <RotateCw />
                  Ricarica
                </button>
              )}
            </div>
            <AddonFrame
              key={`${addonView.key}:${addonReload}`}
              addon={addonView.addon}
              hook={addonView.hook}
              message={null}
              onCompose={composeFromAddon}
              onReply={async () => {
                throw new Error("Qui non c'è un messaggio a cui rispondere");
              }}
              onToast={showToast}
              onClose={() => setView("mail")}
            />
          </section>
        ) : (
          <>
        <section className="sheet">
          <MessageList
            title={currentFolder?.display ?? "Posta in arrivo"}
            subtitle={[currentAccount?.email, unseen > 0 ? `${unseen} da leggere` : ""].filter(Boolean).join(" · ")}
            page={page}
            showRecipient={showRecipient}
            contactOf={contactOf}
            selectedUid={selectedUid}
            loadingOlder={loadingOlder}
            onSelect={(uid) => void openMessage(uid)}
            onDelete={(uid) => void deleteMessage(uid)}
            onLoadOlder={() => void loadOlder()}
          />
        </section>
        <section className="sheet">
          <Reader
            opened={opened}
            summary={selected}
            loading={opening}
            error={openError}
            onReply={(mode) => void reply(mode)}
            onToggleSeen={() => void toggleSeen()}
            onToggleFlag={() => void toggleFlag()}
            onDelete={() => selectedUid !== null && void deleteMessage(selectedUid)}
            onShowImages={(always) => void showImages(always)}
            onOpenAttachment={(a) => void previewAttachment(a)}
            onSaveAttachment={(a) => void saveAttachment(a)}
            onSaveAllAttachments={saveAllAttachments}
            onLink={(url) => void openUrl(url)}
            onMailto={(link) => void api.mailtoParse(link).then((m) => m && newMessage(m))}
            addonButtons={addonButtons}
            onAddonButton={openAddonButton}
            contact={opened ? (contactOf(opened.fromAddr) ?? null) : null}
            canAddContact={
              !!opened?.fromAddr && opened.fromAddr.toLowerCase() !== currentAccount.email.toLowerCase()
            }
            onSaveContact={(input) => saveContact(currentAccount.id, input)}
            suggestCompany={(address) => api.companyFor(currentAccount.id, address)}
            companyNames={companyNames}
            onShowContact={(c) => {
              setContactSel(c.id);
              setView("contacts");
            }}
          />
        </section>
          </>
        )}
        {claudeMounted && (
          <ClaudePanel
            open={claudeOpen}
            status={claudeStatus}
            onRefresh={refreshClaude}
            openSubject={view === "mail" && opened ? opened.subject : null}
            onToast={showToast}
            onClose={toggleClaude}
          />
        )}
      </div>

      {claudeRequests.length > 0 && (
        <ClaudeRequestDialog
          key={claudeRequests[0].id}
          request={claudeRequests[0]}
          total={claudeRequests.length}
          busy={answering}
          canEdit={!compose}
          onAnswer={(answer) => void answerClaude(claudeRequests[0], answer)}
        />
      )}

      {addonPanel && (
        <AddonPanel
          addon={addonPanel.entry.addon}
          hook={addonPanel.entry.hook}
          message={addonPanel.message}
          reloadKey={addonReload}
          onReload={() => void reloadAddons(true)}
          onCompose={(draft) => {
            composeFromAddon(draft);
            setAddonPanel(null);
          }}
          onReply={async (body, all) => {
            await replyFromAddon(addonPanel.message, body, all);
            setAddonPanel(null);
          }}
          onToast={showToast}
          onClose={closeAddonPanel}
        />
      )}

      {addonsOpen && (
        <AddonsDialog installed={addons} onChanged={reloadAddons} onToast={showToast} onClose={closeAddons} />
      )}

      {compose && (
        // Ogni spazio di lavoro scrive dalla sua casella.
        <Compose
          accounts={accounts.filter((a) => a.id === compose.accountId)}
          initial={compose}
          onClose={() => setCompose(null)}
          onSent={onSent}
        />
      )}

      {viewer && (
        <Viewer
          state={viewer}
          craftMissing={(craftStatus?.missing.length ?? 0) > 0}
          onClose={closeViewer}
          onSave={() => void saveAttachment(viewer.attachment)}
          onOpenNative={() => void openViewerNative()}
          onOpenSystem={() => void openAttachment(viewer.attachment)}
          onInstallCraft={() => setCraftSetup(true)}
        />
      )}

      {craftSetup && craftStatus && (
        <ArtCraftSetup
          status={craftStatus}
          onClose={(installed) => {
            setCraftSetup(false);
            if (installed) showToast("ArtCraft è installato");
            void api.craftStatus().then(setCraftStatus);
          }}
        />
      )}

      {addingAccount && (
        <div className="overlay">
          <AccountForm onAdded={(a) => void onAccountAdded(a)} onCancel={() => setAddingAccount(false)} />
        </div>
      )}

      {signatureFor && accounts && (
        <SignaturesDialog
          accounts={accounts}
          accountId={signatureFor.id}
          onClose={() => setSignatureFor(null)}
          onAccounts={setAccounts}
        />
      )}

      {toast && (
        <div className={`toast ${toast.kind}`} role="status">
          {toast.kind === "info" ? <CircleCheck /> : <TriangleAlert />}
          {toast.text}
        </div>
      )}
    </div>
  );
}

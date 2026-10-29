// Tipi e chiamate verso il motore in Rust (src-tauri/src/commands.rs).

import { invoke } from "@tauri-apps/api/core";

export interface Account {
  id: string;
  email: string;
  name: string;
  imapHost: string;
  imapPort: number;
  smtpHost: string;
  smtpPort: number;
  /** Firma dei messaggi nuovi e di risposte e inoltri (id nell'elenco delle firme). */
  signatureNew: string | null;
  signatureReply: string | null;
}

/** Firma dell'elenco: HTML già ripulito, immagini incorporate come data:. */
export interface Signature {
  id: string;
  name: string;
  html: string;
}

export type FolderRole = "inbox" | "sent" | "drafts" | "trash" | "junk" | "archive";

export interface Folder {
  name: string;
  display: string;
  role: FolderRole | null;
  depth: number;
  selectable: boolean;
  unseen: number;
  total: number;
}

export interface Summary {
  uid: number;
  subject: string;
  fromName: string;
  fromAddr: string;
  to: string;
  date: number;
  seen: boolean;
  flagged: boolean;
  answered: boolean;
  hasAttachments: boolean;
  preview: string;
  size: number;
}

export interface MessagePage {
  items: Summary[];
  cached: number;
  total: number;
}

export interface Attachment {
  index: number;
  name: string;
  mime: string;
  size: number;
}

export interface Opened {
  subject: string;
  fromName: string;
  fromAddr: string;
  to: string[];
  cc: string[];
  date: number;
  html: string;
  remoteBlocked: boolean;
  attachments: Attachment[];
  trusted: boolean;
}

export interface MsgRef {
  folder: string;
  uid: number;
}

/** Contatto della rubrica di una casella. */
export interface Contact {
  id: number;
  /** In minuscolo: è la chiave con cui si riconoscono i mittenti. */
  address: string;
  name: string;
  company: string;
  phone: string;
  notes: string;
  created: number;
  /** Il dominio è di un'azienda, non di un servizio di posta comune (Gmail, Libero, PEC…). */
  business: boolean;
}

/** Scheda di un'azienda: le persone ci sono legate dal nome nel loro campo "Azienda". */
export interface Company {
  id: number;
  name: string;
  vat: string;
  phone: string;
  address: string;
  website: string;
  notes: string;
}

/** Scheda da salvare. Senza `id` l'azienda non ha ancora una scheda e `previousName` è il nome delle sue persone. */
export interface CompanyInput {
  id?: number;
  previousName: string;
  name: string;
  vat: string;
  phone: string;
  address: string;
  website: string;
  notes: string;
}

export interface SavedCompany {
  company: Company;
  /** Persone che hanno preso il nome nuovo dell'azienda. */
  contactsRenamed: number;
}

export interface SavedContact {
  contact: Contact;
  /** Colleghi dello stesso dominio aziendale che hanno preso la sua azienda. */
  companyShared: number;
}

/** Contatto da salvare: nuovo se manca `id`. */
export interface ContactInput {
  id?: number;
  address: string;
  name: string;
  company: string;
  phone: string;
  notes: string;
}

export interface ComposeData {
  accountId: string;
  to: string;
  cc: string;
  subject: string;
  body: string;
  /** Citazione o messaggio inoltrato, sotto la firma. */
  quote: string;
  signatureId: string | null;
  inReplyTo: string | null;
  references: string | null;
  reply: MsgRef | null;
  forward: MsgRef | null;
  forwardAttachments: Attachment[];
}

export interface FileInfo {
  path: string;
  name: string;
  size: number;
}

export interface NewAccount {
  name: string;
  email: string;
  password: string;
  imapHost: string;
  imapPort: number;
  smtpHost: string;
  smtpPort: number;
}

export interface Outgoing {
  accountId: string;
  to: string;
  cc: string;
  bcc: string;
  subject: string;
  body: string;
  quote: string;
  signatureId: string | null;
  attachments: string[];
  inReplyTo: string | null;
  references: string | null;
  reply: MsgRef | null;
  forward: MsgRef | null;
}

export interface SendReport {
  savedToSent: boolean;
  warning: string | null;
}

export interface CraftStatus {
  /** Programmi ArtCraft che mancano sul PC. */
  missing: string[];
  total: number;
}

export interface SetupStep {
  app: string;
  stage: "download" | "install" | "done";
  index: number;
  count: number;
  received: number;
  total: number;
}

export interface SetupReport {
  installed: string[];
  failed: string[];
  cancelled: boolean;
}

export interface Preview {
  appId: string;
  appName: string;
  url: string;
  token: string;
  origin: string;
  native: boolean;
}

export interface WebProgress {
  appName: string;
  received: number;
  total: number;
}

// ---- chat interna ----

export interface InternoStatus {
  configured: boolean;
  server: string | null;
  user: { id: number; username: string; displayName: string } | null;
  connected: boolean;
  defaultServer: string;
}

/** Evento della chat di una casella (spazio di lavoro). */
export interface ChatEventFor {
  accountId: string;
  event: ChatEvent;
}

export interface ChatStatusFor {
  accountId: string;
  connected: boolean;
}

export interface ChatUser {
  id: number;
  username: string;
  displayName: string;
  email: string;
  online: boolean;
}

export interface ChatFile {
  id: string;
  name: string;
  mime: string;
  size: number;
}

export interface ChatMessage {
  id: number;
  conversationId: number;
  senderId: number;
  body: string;
  createdAt: number;
  files: ChatFile[];
}

export interface ChatConversation {
  id: number;
  title: string;
  isGroup: boolean;
  members: number[];
  lastMessage: ChatMessage | null;
  unread: number;
  updatedAt: number;
}

export type ChatEvent =
  | { type: "message"; message: ChatMessage }
  | { type: "conversation"; conversationId: number }
  | { type: "read"; conversationId: number; userId: number; messageId: number }
  | { type: "presence"; userId: number; online: boolean };

export interface UploadProgress {
  accountId: string;
  conversationId: number;
  name: string;
  index: number;
  count: number;
  sent: number;
  total: number;
}

export interface MailUpdated {
  accountId: string;
  folder: string;
}

export interface SyncError {
  accountId: string;
  message: string;
}

/** Nuovo messaggio chiesto da un link email (`mailto:`). */
export interface Mailto {
  to: string;
  cc: string;
  bcc: string;
  subject: string;
  body: string;
}

/** Aggiornamento di Postamon già scaricato e verificato. */
export interface UpdateInfo {
  version: string;
  notes: string;
}

export interface UpdateStatus {
  current: string;
  ready: UpdateInfo | null;
}

// ---- add-on ----

export type AddonHookType = "message-button" | "sidebar-view";

export interface AddonHook {
  type: AddonHookType;
  title: string;
  page: string;
}

export interface AddonManifest {
  id: string;
  name: string;
  version: string;
  description: string;
  author: string;
  api: number;
  minPostamon?: string;
  icon?: string;
  permissions: string[];
  hooks: AddonHook[];
}

export interface InstalledAddon {
  id: string;
  version: string;
  enabled: boolean;
  dev: boolean;
  granted: string[];
  manifest: AddonManifest | null;
  /** Cartella del pacchetto sul server locale degli add-on, con la barra finale. */
  baseUrl: string;
  devPath: string | null;
  error: string | null;
}

export interface CatalogItem extends AddonManifest {
  file: string;
  size: number;
  sha256: string;
  installedVersion: string | null;
  dev: boolean;
  update: boolean;
  incompatible: string | null;
  newPermissions: string[];
}

/** Messaggio come lo legge un add-on. */
export interface AddonMessage {
  subject: string;
  from: { name: string; address: string };
  to: { name: string; address: string }[];
  cc: { name: string; address: string }[];
  date: number | null;
  dateText: string;
  text: string;
  attachments: Attachment[];
}

// ---- Claude ----

/** Claude Code sul PC: Postamon ne mostra solo indirizzo e piano dell'accesso. */
export interface ClaudeCli {
  path: string;
  version: string;
  loggedIn: boolean;
  email: string | null;
  plan: string | null;
  authMethod: string | null;
}

export interface ClaudeStatus {
  enabled: boolean;
  cli: ClaudeCli | null;
  desktopInstalled: boolean;
  desktopLinked: boolean;
  codeLinked: boolean;
  /** Modello del pannello, uno di `CLAUDE_MODELS`. */
  model: string;
}

/** Modelli del pannello: nomi brevi di Claude Code, che sceglie l'ultima versione. */
export const CLAUDE_MODELS = [
  { id: "haiku", label: "Haiku" },
  { id: "sonnet", label: "Sonnet" },
  { id: "opus", label: "Opus" },
  { id: "fable", label: "Fable" },
];

export interface ClaudeContext {
  accountId: string | null;
  folder: string | null;
  uid: number | null;
  composeOpen: boolean;
}

export interface ClaudeMessageBrief {
  uid: number;
  subject: string;
  from: string;
  date: number;
}

/** Azione che Claude vuole fare e che l'utente conferma. */
export type ClaudeRequest = { id: number; client: string } & (
  | { kind: "send"; detail: { from: string; signature: string | null; draft: ComposeData & { bcc: string } } }
  | {
      kind: "move";
      detail: { account: string; folder: string; destination: string; messages: ClaudeMessageBrief[] };
    }
  | { kind: "trash"; detail: { account: string; folder: string; permanent: boolean; messages: ClaudeMessageBrief[] } }
  | {
      kind: "addon-install";
      detail: {
        addon: {
          id: string;
          name: string;
          version: string;
          author: string;
          description: string;
          permissions: string[];
          update: boolean;
        };
      };
    }
  | { kind: "addon-uninstall"; detail: { addon: { id: string; name: string; developerMode: boolean } } }
);

export type ClaudeAnswer = "accept" | "reject" | "edit";

/** Evento della conversazione del pannello. */
export type ClaudeChatEvent = { generation: number } & (
  | { kind: "ready"; model: string }
  | { kind: "text"; delta: string }
  | { kind: "tool"; id: string; name: string; input: Record<string, unknown> | null }
  | { kind: "toolResult"; id: string; isError: boolean }
  | { kind: "done"; error: string | null }
  | { kind: "limit"; resetsAt: number | null }
  | { kind: "exit"; error: string | null }
);

/** Cosa Claude vuole mostrare all'utente in Postamon. */
export interface ClaudeOpen {
  accountId?: string;
  folder?: string;
  uid?: number | null;
  addon?: string;
}

export const api = {
  accounts: () => invoke<Account[]>("accounts_list"),
  addAccount: (input: NewAccount) => invoke<Account>("account_add", { input }),
  removeAccount: (accountId: string) => invoke<void>("account_remove", { accountId }),
  setSignatures: (accountId: string, signatureNew: string | null, signatureReply: string | null) =>
    invoke<Account>("account_set_signatures", { accountId, signatureNew, signatureReply }),
  signatures: () => invoke<Signature[]>("signatures_list"),
  saveSignature: (id: string | null, name: string, html: string) =>
    invoke<Signature>("signature_save", { id, name, html }),
  deleteSignature: (id: string) => invoke<Account[]>("signature_delete", { id }),
  cleanSignature: (html: string) => invoke<string>("signature_clean", { html }),
  signatureImage: (path: string) => invoke<string>("signature_image", { path }),
  outlookSignatures: () => invoke<string[]>("signatures_outlook"),
  importOutlookSignatures: (names: string[]) => invoke<Signature[]>("signatures_import_outlook", { names }),
  folders: (accountId: string) => invoke<Folder[]>("folders_list", { accountId }),
  sync: (accountId: string, folder: string, refreshFolders: boolean) =>
    invoke<{ newMessages: number }>("sync", { accountId, folder, refreshFolders }),
  messages: (accountId: string, folder: string, limit: number) =>
    invoke<MessagePage>("messages_list", { accountId, folder, limit }),
  loadOlder: (accountId: string, folder: string) => invoke<number>("messages_load_older", { accountId, folder }),
  open: (accountId: string, folder: string, uid: number, allowRemote: boolean) =>
    invoke<Opened>("message_open", { accountId, folder, uid, allowRemote }),
  setSeen: (accountId: string, folder: string, uid: number, seen: boolean) =>
    invoke<void>("message_set_seen", { accountId, folder, uid, seen }),
  setFlagged: (accountId: string, folder: string, uid: number, flagged: boolean) =>
    invoke<void>("message_set_flagged", { accountId, folder, uid, flagged }),
  remove: (accountId: string, folder: string, uid: number) =>
    invoke<void>("message_delete", { accountId, folder, uid }),
  saveAttachment: (accountId: string, folder: string, uid: number, index: number, path: string) =>
    invoke<void>("attachment_save", { accountId, folder, uid, index, path }),
  /** Salva tutti gli allegati in `dir` senza sovrascrivere niente; restituisce quanti ne ha salvati. */
  saveAllAttachments: (accountId: string, folder: string, uid: number, dir: string) =>
    invoke<number>("attachments_save_all", { accountId, folder, uid, dir }),
  openAttachment: (accountId: string, folder: string, uid: number, index: number) =>
    invoke<void>("attachment_open", { accountId, folder, uid, index }),
  trustSender: (address: string) => invoke<void>("sender_trust", { address }),
  contacts: (query: string) => invoke<string[]>("contacts_suggest", { query }),
  addressBook: (accountId: string) => invoke<Contact[]>("address_book_list", { accountId }),
  saveContact: (accountId: string, contact: ContactInput) =>
    invoke<SavedContact>("address_book_save", { accountId, contact }),
  companyFor: (accountId: string, address: string) =>
    invoke<string | null>("address_book_company", { accountId, address }),
  deleteContact: (accountId: string, id: number) => invoke<void>("address_book_delete", { accountId, id }),
  companies: (accountId: string) => invoke<Company[]>("address_book_companies", { accountId }),
  saveCompany: (accountId: string, company: CompanyInput) =>
    invoke<SavedCompany>("address_book_company_save", { accountId, company }),
  filesInfo: (paths: string[]) => invoke<FileInfo[]>("files_info", { paths }),
  composeFrom: (accountId: string, folder: string, uid: number, mode: "reply" | "replyAll" | "forward") =>
    invoke<ComposeData>("compose_from", { accountId, folder, uid, mode }),
  send: (input: Outgoing) => invoke<SendReport>("message_send", { input }),
  craftStatus: () => invoke<CraftStatus>("craft_status"),
  craftInstall: () => invoke<SetupReport>("craft_install"),
  preview: (accountId: string, folder: string, uid: number, index: number) =>
    invoke<Preview | null>("attachment_preview", { accountId, folder, uid, index }),
  previewClose: (token: string) => invoke<void>("preview_close", { token }),
  openNative: (accountId: string, folder: string, uid: number, index: number) =>
    invoke<void>("attachment_open_native", { accountId, folder, uid, index }),

  // La chat ha una sessione per casella: ogni chiamata dice di quale.
  internoStatus: (accountId: string) => invoke<InternoStatus>("interno_status", { accountId }),
  internoLogin: (accountId: string, server: string) =>
    invoke<InternoStatus["user"]>("interno_login", { accountId, server }),
  internoLogout: (accountId: string) => invoke<void>("interno_logout", { accountId }),
  internoGet: <T>(accountId: string, path: string) =>
    invoke<T>("interno_call", { accountId, method: "GET", path, body: null }),
  internoPost: <T>(accountId: string, path: string, body: unknown) =>
    invoke<T>("interno_call", { accountId, method: "POST", path, body }),
  internoSend: (accountId: string, conversationId: number, body: string, paths: string[]) =>
    invoke<ChatMessage>("interno_send", { accountId, conversationId, body, paths }),
  internoFileSave: (accountId: string, fileId: string, path: string) =>
    invoke<void>("interno_file_save", { accountId, fileId, path }),
  internoFilePreview: (accountId: string, file: ChatFile) =>
    invoke<Preview | null>("interno_file_preview", {
      accountId,
      fileId: file.id,
      name: file.name,
      mime: file.mime,
      size: file.size,
    }),
  internoFileOpen: (accountId: string, file: ChatFile, native: boolean) =>
    invoke<void>("interno_file_open", { accountId, fileId: file.id, name: file.name, native }),

  /** Link email con cui Windows ha aperto Postamon (una volta sola). */
  mailtoTake: () => invoke<Mailto | null>("mailto_take"),
  mailtoParse: (link: string) => invoke<Mailto | null>("mailto_parse", { link }),
  /** Pagina di Windows dove si sceglie Postamon per i link email. */
  mailtoSettings: () => invoke<void>("mailto_settings"),

  updateStatus: () => invoke<UpdateStatus>("update_status"),
  /** Chiude Postamon: l'installer lo riapre aggiornato. */
  updateInstall: () => invoke<void>("update_install"),

  addonsInstalled: () => invoke<InstalledAddon[]>("addons_installed"),
  addonsCatalog: () => invoke<CatalogItem[]>("addons_catalog"),
  /** `permissions`: quelli mostrati all'utente; se il catalogo ne chiede altri non si installa. */
  addonInstall: (id: string, permissions: string[]) => invoke<void>("addon_install", { id, permissions }),
  addonUninstall: (id: string) => invoke<void>("addon_uninstall", { id }),
  addonSetEnabled: (id: string, enabled: boolean) => invoke<void>("addon_set_enabled", { id, enabled }),
  addonDevAdd: (path: string) => invoke<string>("addon_dev_add", { path }),
  addonMessage: (addonId: string, accountId: string, folder: string, uid: number) =>
    invoke<AddonMessage>("addon_message", { addonId, accountId, folder, uid }),
  addonStorage: (addonId: string, op: "get" | "set" | "remove" | "keys", key?: string, value?: unknown) =>
    invoke<unknown>("addon_storage", { addonId, op, key: key ?? null, value: value ?? null }),

  claudeStatus: () => invoke<ClaudeStatus>("claude_status"),
  claudeSetEnabled: (enabled: boolean) => invoke<void>("claude_set_enabled", { enabled }),
  /** Vale dal prossimo messaggio, nella stessa conversazione. */
  claudeSetModel: (model: string) => invoke<void>("claude_set_model", { model }),
  /** Apre la finestra di accesso di Claude Code. */
  claudeLogin: () => invoke<void>("claude_login"),
  claudeLinkDesktop: (on: boolean) => invoke<void>("claude_link_desktop", { on }),
  claudeLinkCode: (on: boolean) => invoke<void>("claude_link_code", { on }),
  claudeContext: (context: ClaudeContext) => invoke<void>("claude_context", { context }),
  claudeRequests: () => invoke<ClaudeRequest[]>("claude_requests"),
  claudeAnswer: (id: number, answer: ClaudeAnswer) => invoke<string>("claude_answer", { id, answer }),
  /** Restituisce il numero della conversazione negli eventi `claude-chat`. */
  claudeChatSend: (text: string) => invoke<number>("claude_chat_send", { text }),
  claudeChatStop: () => invoke<void>("claude_chat_stop"),
  claudeChatReset: () => invoke<void>("claude_chat_reset"),
};

/** Cosa sta facendo Claude con uno strumento di Postamon, in parole semplici. */
export function claudeToolText(name: string, input: Record<string, unknown> | null): string {
  const query = typeof input?.query === "string" ? input.query : "";
  switch (name) {
    case "list_accounts":
      return "Guarda le caselle";
    case "list_folders":
      return "Guarda le cartelle";
    case "list_messages":
      return "Scorre i messaggi";
    case "search_messages":
      return query ? `Cerca «${query}»` : "Cerca nella posta";
    case "read_message":
      return "Legge un messaggio";
    case "current_message":
      return "Guarda il messaggio aperto";
    case "compose_email":
      return "Prepara un messaggio";
    case "send_email":
      return "Chiede di inviare un messaggio";
    case "mark_messages":
      return "Segna dei messaggi";
    case "move_messages":
      return "Chiede di spostare dei messaggi";
    case "trash_messages":
      return "Chiede di cestinare dei messaggi";
    case "request_status":
      return "Controlla una conferma";
    case "open_in_postamon":
      return "Ti mostra qualcosa in Postamon";
    case "list_addons":
      return "Guarda gli add-on";
    case "install_addon":
      return "Chiede di installare un add-on";
    case "uninstall_addon":
      return "Chiede di togliere un add-on";
    case "set_addon_enabled":
      return "Attiva o disattiva un add-on";
    default:
      return name;
  }
}

/** Cosa può fare un add-on con un permesso, in parole semplici. */
export function permissionText(p: string): { text: string; warn: boolean } {
  if (p === "message.read")
    return { text: "Legge il messaggio aperto: mittente, destinatari, oggetto, testo e nomi degli allegati", warn: false };
  if (p === "compose") return { text: "Prepara messaggi e risposte, che invii sempre tu", warn: false };
  if (p.startsWith("network:")) return { text: `Si collega a internet, solo a ${p.slice(8)}`, warn: true };
  return { text: p, warn: true };
}

/** Gli errori del motore arrivano come stringhe già pronte da mostrare. */
export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}

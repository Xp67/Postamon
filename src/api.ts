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
  signature: string;
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

export interface ComposeData {
  accountId: string;
  to: string;
  cc: string;
  subject: string;
  body: string;
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

/** Aggiornamento di Postamon già scaricato e verificato. */
export interface UpdateInfo {
  version: string;
  notes: string;
}

export interface UpdateStatus {
  current: string;
  ready: UpdateInfo | null;
}

export const api = {
  accounts: () => invoke<Account[]>("accounts_list"),
  addAccount: (input: NewAccount) => invoke<Account>("account_add", { input }),
  removeAccount: (accountId: string) => invoke<void>("account_remove", { accountId }),
  setSignature: (accountId: string, signature: string) =>
    invoke<Account>("account_set_signature", { accountId, signature }),
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
  openAttachment: (accountId: string, folder: string, uid: number, index: number) =>
    invoke<void>("attachment_open", { accountId, folder, uid, index }),
  trustSender: (address: string) => invoke<void>("sender_trust", { address }),
  contacts: (query: string) => invoke<string[]>("contacts_suggest", { query }),
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

  updateStatus: () => invoke<UpdateStatus>("update_status"),
  /** Chiude Postamon: l'installer lo riapre aggiornato. */
  updateInstall: () => invoke<void>("update_install"),
};

/** Gli errori del motore arrivano come stringhe già pronte da mostrare. */
export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}

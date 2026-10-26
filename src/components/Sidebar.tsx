import { useEffect, useRef, useState } from "react";
import {
  Archive,
  Ellipsis,
  FileText,
  Folder as FolderIcon,
  Inbox,
  LoaderCircle,
  MessagesSquare,
  Send,
  ShieldAlert,
  Signature,
  Trash,
  TriangleAlert,
  X,
} from "lucide-react";
import type { Account, Folder, FolderRole } from "../api";

const ROLE_ICONS: Record<FolderRole, typeof Inbox> = {
  inbox: Inbox,
  sent: Send,
  drafts: FileText,
  trash: Trash,
  junk: ShieldAlert,
  archive: Archive,
};

interface Props {
  /** Casella dello spazio di lavoro aperto. */
  account: Account;
  folders: Folder[];
  /** Cartella aperta (nessuna se è aperta la chat). */
  folder: string | null;
  syncing: boolean;
  error: string | undefined;
  chatActive: boolean;
  chatUnread: number;
  onSelect: (folder: string) => void;
  onOpenChat: () => void;
  onEditSignature: (account: Account) => void;
  onRemoveAccount: (account: Account) => void;
}

/** Barra laterale dello spazio di lavoro di una casella: chat e cartelle. */
export function Sidebar({
  account,
  folders,
  folder,
  syncing,
  error,
  chatActive,
  chatUnread,
  onSelect,
  onOpenChat,
  onEditSignature,
  onRemoveAccount,
}: Props) {
  const [menu, setMenu] = useState(false);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const head = useRef<HTMLDivElement>(null);

  useEffect(() => {
    setMenu(false);
    setConfirmRemove(false);
  }, [account.id]);

  useEffect(() => {
    if (!menu) return;
    const close = (e: MouseEvent) => {
      if (!head.current?.contains(e.target as Node)) {
        setMenu(false);
        setConfirmRemove(false);
      }
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [menu]);

  return (
    <nav className="sidebar" aria-label={`Spazio di ${account.email}`}>
      <div className="account-head workspace-head" ref={head}>
        <div className="account-title">
          <div className="account-name">{account.name || account.email}</div>
          <div className="account-email">{account.email}</div>
        </div>
        {syncing && <LoaderCircle className="spin" size={15} color="var(--ink-3)" aria-label="Aggiornamento" />}
        {error && !syncing && (
          <span className="account-alert" title={`Server non raggiungibile: ${error}`}>
            <TriangleAlert />
          </span>
        )}
        <button className="btn btn-ghost icon-btn" aria-label="Opzioni casella" onClick={() => setMenu(!menu)}>
          <Ellipsis />
        </button>
        {menu && (
          <div className="menu" role="menu">
            <button
              onClick={() => {
                setMenu(false);
                onEditSignature(account);
              }}
            >
              <Signature />
              Firma
            </button>
            {confirmRemove ? (
              <button className="danger" onClick={() => onRemoveAccount(account)}>
                <X />
                Conferma: togli la casella
              </button>
            ) : (
              <button className="danger" onClick={() => setConfirmRemove(true)}>
                <X />
                Togli casella da Postamon
              </button>
            )}
          </div>
        )}
      </div>

      <button className={`folder chat-entry ${chatActive ? "active" : ""}`} onClick={onOpenChat}>
        <MessagesSquare />
        <span className="folder-label">Chat interna</span>
        {chatUnread > 0 && <span className="count">{chatUnread}</span>}
      </button>

      <div className="sidebar-section">Posta</div>
      {folders.map((f) => {
        const Icon = (f.role && ROLE_ICONS[f.role]) || FolderIcon;
        const active = !chatActive && folder === f.name;
        const showCount = f.unseen > 0 && f.role !== "sent" && f.role !== "trash" && f.role !== "junk";
        return (
          <button
            key={f.name}
            className={`folder ${active ? "active" : ""}`}
            style={{ paddingLeft: 10 + f.depth * 16 }}
            disabled={!f.selectable}
            onClick={() => onSelect(f.name)}
            title={f.display}
          >
            <Icon />
            <span className="folder-label">{f.display}</span>
            {showCount && <span className="count">{f.unseen}</span>}
          </button>
        );
      })}
    </nav>
  );
}

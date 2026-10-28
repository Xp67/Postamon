import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { Flag, Inbox, LoaderCircle, Paperclip, Reply, Search } from "lucide-react";
import type { Contact, MessagePage, Summary } from "../api";
import { listDate, senderLabel } from "../format";
import { contactLabel } from "../book";

interface Props {
  title: string;
  subtitle: string;
  page: MessagePage | null;
  /** Nella posta inviata si mostra il destinatario invece del mittente. */
  showRecipient: boolean;
  /** Il mittente nella rubrica della casella, se c'è. */
  contactOf: (address: string) => Contact | undefined;
  selectedUid: number | null;
  loadingOlder: boolean;
  onSelect: (uid: number) => void;
  onDelete: (uid: number) => void;
  onLoadOlder: () => void;
}

export function MessageList({
  title,
  subtitle,
  page,
  showRecipient,
  contactOf,
  selectedUid,
  loadingOlder,
  onSelect,
  onDelete,
  onLoadOlder,
}: Props) {
  const [query, setQuery] = useState("");
  const list = useRef<HTMLDivElement>(null);

  useEffect(() => setQuery(""), [title, subtitle]);

  const items = useMemo(() => {
    const all = page?.items ?? [];
    const q = query.trim().toLowerCase();
    if (!q) return all;
    return all.filter((m) =>
      [m.subject, m.fromName, m.fromAddr, m.to, m.preview, contactOf(m.fromAddr)?.name ?? ""].some((v) =>
        v.toLowerCase().includes(q),
      ),
    );
  }, [page, query, contactOf]);

  // Il messaggio selezionato resta visibile quando si scorre con la tastiera.
  useEffect(() => {
    list.current?.querySelector(".row.selected")?.scrollIntoView({ block: "nearest" });
  }, [selectedUid]);

  const onKey = (e: KeyboardEvent) => {
    if (!items.length) return;
    const idx = items.findIndex((m) => m.uid === selectedUid);
    if (e.key === "ArrowDown") {
      e.preventDefault();
      onSelect(items[Math.min(items.length - 1, idx + 1)].uid);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      onSelect(items[Math.max(0, idx - 1)].uid);
    } else if (e.key === "Delete" && selectedUid !== null) {
      e.preventDefault();
      onDelete(selectedUid);
    }
  };

  const moreOnServer = page ? page.total > page.cached || page.cached > page.items.length : false;

  return (
    <>
      <div className="list-head">
        <div className="list-title">
          <h2>{title}</h2>
          <span className="sub">{subtitle}</span>
        </div>
        <label className="search">
          <Search />
          <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Cerca in questa cartella" />
        </label>
      </div>
      <div className="list" ref={list} tabIndex={0} onKeyDown={onKey}>
        {page === null && (
          <div className="empty">
            <LoaderCircle className="spin" />
          </div>
        )}
        {page !== null && items.length === 0 && (
          <div className="empty">
            <div>
              <Inbox />
              <div>{query ? "Nessun messaggio trovato" : "Nessun messaggio"}</div>
            </div>
          </div>
        )}
        {items.map((m) => (
          <Row
            key={m.uid}
            message={m}
            showRecipient={showRecipient}
            contact={showRecipient ? undefined : contactOf(m.fromAddr)}
            selected={m.uid === selectedUid}
            onClick={() => onSelect(m.uid)}
          />
        ))}
        {page !== null && moreOnServer && !query && (
          <div className="list-foot">
            <button className="btn btn-small" onClick={onLoadOlder} disabled={loadingOlder}>
              {loadingOlder && <LoaderCircle className="spin" />}
              Carica messaggi precedenti
            </button>
          </div>
        )}
      </div>
    </>
  );
}

function Row({
  message: m,
  showRecipient,
  contact,
  selected,
  onClick,
}: {
  message: Summary;
  showRecipient: boolean;
  contact: Contact | undefined;
  selected: boolean;
  onClick: () => void;
}) {
  const who = showRecipient
    ? `A: ${m.to || "(nessuno)"}`
    : contact
      ? contactLabel(contact)
      : senderLabel(m.fromName, m.fromAddr);
  return (
    <div className={`row ${m.seen ? "" : "unread"} ${selected ? "selected" : ""}`} onClick={onClick}>
      {contact && <span className="contact-mark" role="img" aria-label="In rubrica" title="In rubrica" />}
      <span className="dot" />
      <span className="row-from" title={m.fromAddr}>
        {who}
      </span>
      <span className="row-date">{listDate(m.date)}</span>
      <span />
      <span className="row-subject">
        <span>{m.subject || "(senza oggetto)"}</span>
        <span className="row-icons">
          {m.answered && <Reply aria-label="Risposto" />}
          {m.hasAttachments && <Paperclip aria-label="Con allegati" />}
          {m.flagged && <Flag className="flagged" aria-label="Contrassegnato" />}
        </span>
      </span>
      <span />
      <span className="row-preview">{m.preview || " "}</span>
    </div>
  );
}

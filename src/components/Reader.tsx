import { useRef, useState } from "react";
import {
  Download,
  File,
  FileArchive,
  FileImage,
  FileSpreadsheet,
  FileText,
  Flag,
  Forward,
  ImageOff,
  LoaderCircle,
  Mail,
  MailOpen,
  Reply,
  ReplyAll,
  Trash,
  TriangleAlert,
} from "lucide-react";
import type { Attachment, Opened, Summary } from "../api";
import { avatarHue, fileSize, fullDate, initials, senderLabel } from "../format";
import { Help } from "./Help";

interface Props {
  opened: Opened | null;
  summary: Summary | null;
  loading: boolean;
  error: string;
  onReply: (mode: "reply" | "replyAll" | "forward") => void;
  onToggleSeen: () => void;
  onToggleFlag: () => void;
  onDelete: () => void;
  onShowImages: (always: boolean) => void;
  onOpenAttachment: (a: Attachment) => void;
  onSaveAttachment: (a: Attachment) => void;
  onLink: (url: string) => void;
  onMailto: (address: string) => void;
}

function attachmentIcon(a: Attachment) {
  if (a.mime.startsWith("image/")) return FileImage;
  if (/zip|rar|7z|compressed/.test(a.mime)) return FileArchive;
  if (/sheet|excel|csv/.test(a.mime)) return FileSpreadsheet;
  if (/pdf|word|text|document/.test(a.mime)) return FileText;
  return File;
}

export function Reader(props: Props) {
  const { opened, summary, loading, error } = props;
  const frame = useRef<HTMLIFrameElement>(null);
  const [hoverUrl, setHoverUrl] = useState("");

  if (!opened) {
    return (
      <div className="reader">
        {loading ? (
          <div className="reader-loading">
            <LoaderCircle className="spin" />
          </div>
        ) : error ? (
          <div className="empty">
            <div>
              <TriangleAlert />
              <div>{error}</div>
            </div>
          </div>
        ) : (
          <div className="empty">
            <div>
              <Mail />
              <div>Seleziona un messaggio</div>
            </div>
          </div>
        )}
      </div>
    );
  }

  // L'email sta in un iframe senza script: i clic sui link li gestisce
  // l'applicazione, che li apre nel browser o nella finestra di scrittura.
  const wireLinks = () => {
    const doc = frame.current?.contentDocument;
    if (!doc) return;
    const anchorOf = (target: EventTarget | null) =>
      (target as Element | null)?.closest?.("a[href]") as HTMLAnchorElement | null;
    doc.addEventListener("click", (e) => {
      const a = anchorOf(e.target);
      if (!a) return;
      const href = a.getAttribute("href") ?? "";
      if (href.startsWith("#")) return;
      e.preventDefault();
      if (/^https?:/i.test(href)) props.onLink(href);
      else if (/^mailto:/i.test(href)) props.onMailto(decodeURIComponent(href.slice(7).split("?")[0]));
    });
    doc.addEventListener("mouseover", (e) => {
      const a = anchorOf(e.target);
      setHoverUrl(a ? (a.getAttribute("href") ?? "") : "");
    });
  };

  const hue = avatarHue(opened.fromAddr);
  const flagged = summary?.flagged ?? false;
  const seen = summary?.seen ?? true;

  return (
    <div className="reader">
      <div className="reader-head selectable">
        <h1 className="reader-subject">{opened.subject || "(senza oggetto)"}</h1>
        <div className="sender">
          <div className="avatar" style={{ background: `hsl(${hue} 42% 42%)` }}>
            {initials(opened.fromName, opened.fromAddr)}
          </div>
          <div className="sender-main">
            <div className="sender-name">{senderLabel(opened.fromName, opened.fromAddr)}</div>
            <div className="sender-addr">{opened.fromAddr}</div>
          </div>
          <div className="sender-date">{fullDate(opened.date)}</div>
        </div>
        {(opened.to.length > 0 || opened.cc.length > 0) && (
          <div className="recipients">
            {opened.to.length > 0 && (
              <div>
                <b>A:</b>
                {opened.to.join(", ")}
              </div>
            )}
            {opened.cc.length > 0 && (
              <div>
                <b>Cc:</b>
                {opened.cc.join(", ")}
              </div>
            )}
          </div>
        )}
      </div>

      <div className="toolbar">
        <button className="btn" onClick={() => props.onReply("reply")}>
          <Reply />
          Rispondi
        </button>
        <button className="btn" onClick={() => props.onReply("replyAll")}>
          <ReplyAll />
          Rispondi a tutti
        </button>
        <button className="btn" onClick={() => props.onReply("forward")}>
          <Forward />
          Inoltra
        </button>
        <span className="spacer" />
        <button
          className="btn btn-ghost icon-btn"
          onClick={props.onToggleSeen}
          title={seen ? "Segna come da leggere" : "Segna come letto"}
          aria-label={seen ? "Segna come da leggere" : "Segna come letto"}
        >
          {seen ? <Mail /> : <MailOpen />}
        </button>
        <button
          className="btn btn-ghost icon-btn"
          onClick={props.onToggleFlag}
          title={flagged ? "Togli il contrassegno" : "Contrassegna"}
          aria-label={flagged ? "Togli il contrassegno" : "Contrassegna"}
          style={flagged ? { color: "var(--danger)" } : undefined}
        >
          <Flag />
        </button>
        <button className="btn btn-danger" onClick={props.onDelete}>
          <Trash />
          Elimina
        </button>
      </div>

      {opened.remoteBlocked && (
        <div className="banner">
          <ImageOff />
          <span className="grow">
            Immagini esterne bloccate
            <Help>
              Le immagini scaricate da internet dicono al mittente quando e dove hai aperto il messaggio. Per questo
              restano bloccate finché non le chiedi tu.
            </Help>
          </span>
          <button className="btn btn-small" onClick={() => props.onShowImages(false)}>
            Mostra immagini
          </button>
          <button className="btn btn-small" onClick={() => props.onShowImages(true)}>
            Sempre da questo mittente
          </button>
        </div>
      )}

      {opened.attachments.length > 0 && (
        <div className="attachments">
          {opened.attachments.map((a) => {
            const Icon = attachmentIcon(a);
            return (
              <div className="chip" key={a.index}>
                <button className="chip-open" onClick={() => props.onOpenAttachment(a)} title={`Apri ${a.name}`}>
                  <Icon className="kind" />
                  <span className="chip-text">
                    <span className="chip-name">{a.name}</span>
                    <span className="chip-size">{fileSize(a.size)}</span>
                  </span>
                </button>
                <button
                  className="btn btn-ghost icon-btn"
                  onClick={() => props.onSaveAttachment(a)}
                  title="Salva"
                  aria-label={`Salva ${a.name}`}
                >
                  <Download />
                </button>
              </div>
            );
          })}
        </div>
      )}

      <iframe
        ref={frame}
        className="mail-frame"
        title="Contenuto del messaggio"
        sandbox="allow-same-origin"
        srcDoc={opened.html}
        onLoad={wireLinks}
      />
      {hoverUrl && <div className="link-status">{hoverUrl}</div>}
    </div>
  );
}

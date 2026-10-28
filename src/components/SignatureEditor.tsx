import { useEffect, useRef, useState, type ClipboardEvent, type MouseEvent, type ReactNode } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  Baseline,
  Bold,
  Check,
  CodeXml,
  Columns2,
  Eraser,
  ImagePlus,
  Italic,
  Link,
  SeparatorHorizontal,
  TextAlignCenter,
  TextAlignEnd,
  TextAlignStart,
  Trash,
  Underline,
  Unlink,
} from "lucide-react";
import { api, errorText } from "../api";
import { Help } from "./Help";

const FONTS = ["Segoe UI", "Calibri", "Arial", "Verdana", "Tahoma", "Georgia", "Times New Roman", "Courier New"];
const SIZES: [string, number][] = [
  ["Piccolo", 9],
  ["Normale", 11],
  ["Medio", 13],
  ["Grande", 16],
  ["Molto grande", 20],
];
/** Come in signatures.rs: oltre questa misura un'immagine non è da firma. */
const MAX_IMAGE = 2 * 1024 * 1024;
const IMAGE_TYPES = ["image/png", "image/jpeg", "image/gif"];
/** Larghezza di partenza delle immagini nuove: un logo, non una foto a tutta pagina. */
const IMAGE_START_WIDTH = 220;

/** Logo a sinistra, recapiti a destra dopo una riga verticale: lo schema delle firme di Outlook. */
const COLUMNS_HTML =
  '<table style="border-collapse:collapse"><tbody><tr>' +
  '<td style="vertical-align:middle;padding:0 14px 0 0">Logo</td>' +
  '<td style="vertical-align:middle;padding:0 0 0 14px;border-left:2px solid #454545">' +
  "<div><b>Nome e cognome</b></div><div>Ruolo</div><div>Telefono</div></td>" +
  "</tr></tbody></table><div><br></div>";

/** HTML di una firma come arriverà: stesso carattere di base dei messaggi inviati. */
export function SignatureHtml({ html }: { html: string }) {
  return <div className="sig-html" dangerouslySetInnerHTML={{ __html: html }} />;
}

interface Props {
  /** HTML di partenza: si rilegge solo quando cambia `docKey` (si apre un'altra firma). */
  html: string;
  docKey: string;
  onChange: (html: string) => void;
  onError: (text: string) => void;
}

/** Editor visuale della firma, con il codice HTML a portata di mano. */
export function SignatureEditor({ html, docKey, onChange, onError }: Props) {
  const surface = useRef<HTMLDivElement>(null);
  const colorInput = useRef<HTMLInputElement>(null);
  const range = useRef<Range | null>(null);
  const [source, setSource] = useState<string | null>(null);
  const [picked, setPicked] = useState<HTMLImageElement | null>(null);
  // Larghezza scritta a mano: si applica con Invio o uscendo dal campo, non a ogni cifra.
  const [widthText, setWidthText] = useState("");
  const [link, setLink] = useState<string | null>(null);
  const [, setTick] = useState(0);

  // Un'altra firma: si riparte dal suo HTML.
  useEffect(() => {
    if (surface.current) surface.current.innerHTML = html;
    range.current = null;
    setSource(null);
    setPicked(null);
    setLink(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [docKey]);

  // Ultima selezione dentro la firma: i comandi della barra la ritrovano anche
  // dopo un clic su un menu, sul colore o sul campo del link.
  useEffect(() => {
    const onSelection = () => {
      const sel = document.getSelection();
      if (sel && sel.rangeCount > 0 && surface.current?.contains(sel.anchorNode)) {
        range.current = sel.getRangeAt(0).cloneRange();
        setTick((t) => t + 1);
      }
    };
    document.addEventListener("selectionchange", onSelection);
    return () => document.removeEventListener("selectionchange", onSelection);
  }, []);

  const emit = () => {
    const el = surface.current;
    if (el) onChange(el.innerHTML.replace(/ class="sig-picked"/g, ""));
  };

  const restore = () => {
    const el = surface.current;
    if (!el) return;
    el.focus();
    const sel = document.getSelection();
    if (range.current && sel) {
      sel.removeAllRanges();
      sel.addRange(range.current);
    }
  };

  const exec = (command: string, value?: string, css = true) => {
    restore();
    document.execCommand("defaultParagraphSeparator", false, "div");
    document.execCommand("styleWithCSS", false, css ? "true" : "false");
    document.execCommand(command, false, value);
    emit();
  };

  // Il colore si applica quando si chiude la tavolozza, non a ogni passaggio.
  useEffect(() => {
    const input = colorInput.current;
    if (!input) return;
    const apply = () => exec("foreColor", input.value);
    input.addEventListener("change", apply);
    return () => input.removeEventListener("change", apply);
  });

  // execCommand conosce solo 7 misure fisse: si passa per la 7 e la si
  // sostituisce con i punti voluti.
  const setSize = (pt: number) => {
    exec("fontSize", "7", false);
    surface.current?.querySelectorAll('font[size="7"]').forEach((font) => {
      const span = document.createElement("span");
      span.style.fontSize = `${pt}pt`;
      while (font.firstChild) span.appendChild(font.firstChild);
      font.replaceWith(span);
    });
    emit();
  };

  const insertImage = (src: string) => {
    exec("insertImage", src);
    surface.current?.querySelectorAll<HTMLImageElement>("img:not([width])").forEach((img) => {
      const fit = () => {
        if (!img.naturalWidth) return;
        resize(img, Math.min(img.naturalWidth, IMAGE_START_WIDTH));
      };
      if (img.complete) fit();
      else img.addEventListener("load", fit, { once: true });
    });
  };

  const pickImage = async () => {
    const path = await openDialog({
      title: "Immagine per la firma",
      filters: [{ name: "Immagini", extensions: ["png", "jpg", "jpeg", "gif"] }],
    });
    if (typeof path !== "string") return;
    try {
      insertImage(await api.signatureImage(path));
    } catch (e) {
      onError(errorText(e));
    }
  };

  // Un'immagine incollata (logo copiato, ritaglio dello schermo) entra come le
  // altre; testo e HTML li incolla il browser e il motore li ripulisce al salvataggio.
  const onPaste = (e: ClipboardEvent) => {
    const file = Array.from(e.clipboardData.files).find((f) => f.type.startsWith("image/"));
    if (!file) return;
    e.preventDefault();
    if (!IMAGE_TYPES.includes(file.type)) {
      onError("Per la firma servono immagini PNG, JPG o GIF");
      return;
    }
    if (file.size > MAX_IMAGE) {
      onError("Immagine troppo grande per una firma (massimo 2 MB): riducila prima di incollarla");
      return;
    }
    const reader = new FileReader();
    reader.onload = () => typeof reader.result === "string" && insertImage(reader.result);
    reader.readAsDataURL(file);
  };

  const resize = (img: HTMLImageElement, width: number) => {
    const w = Math.max(8, Math.round(width));
    img.setAttribute("width", String(w));
    img.removeAttribute("height");
    img.style.width = `${w}px`;
    img.style.height = "auto";
    setWidthText(String(w));
    emit();
  };

  const applyWidth = () => {
    if (!picked) return;
    const w = Number(widthText);
    if (w >= 8) resize(picked, w);
    else setWidthText(String(picked.width));
  };

  // Clic su un'immagine: la si seleziona tutta, così Link e Canc valgono per lei.
  const onClick = (e: MouseEvent) => {
    surface.current?.querySelectorAll(".sig-picked").forEach((i) => i.classList.remove("sig-picked"));
    const target = e.target;
    if (!(target instanceof HTMLImageElement)) {
      setPicked(null);
      return;
    }
    target.classList.add("sig-picked");
    const r = document.createRange();
    r.selectNode(target);
    const sel = document.getSelection();
    sel?.removeAllRanges();
    sel?.addRange(r);
    range.current = r;
    setPicked(target);
    setWidthText(String(target.width));
  };

  const removePicked = () => {
    picked?.remove();
    setPicked(null);
    emit();
  };

  const currentLink = (): HTMLAnchorElement | null => {
    const node = range.current?.commonAncestorContainer;
    const el = node instanceof Element ? node : node?.parentElement;
    const a = el?.closest("a");
    return a && surface.current?.contains(a) ? a : null;
  };

  const applyLink = () => {
    const typed = (link ?? "").trim();
    const url = normalizeUrl(typed);
    setLink(null);
    if (!url) return;
    const a = currentLink();
    if (a) {
      a.setAttribute("href", url);
      emit();
    } else if (!range.current || range.current.collapsed) {
      // Senza testo selezionato il link si scrive com'è stato digitato.
      exec("insertHTML", `<a href="${escapeHtml(url)}">${escapeHtml(typed)}</a>`);
    } else {
      exec("createLink", url);
    }
  };

  const removeLink = () => {
    const a = currentLink();
    setLink(null);
    if (!a) return;
    a.replaceWith(...Array.from(a.childNodes));
    emit();
  };

  const toggleSource = async () => {
    if (source === null) {
      setPicked(null);
      setLink(null);
      const current = (surface.current?.innerHTML ?? "").replace(/ class="sig-picked"/g, "");
      setSource(current.replace(/(<\/(div|p|tr|table|tbody)>)/g, "$1\n"));
      return;
    }
    try {
      const clean = await api.cleanSignature(source);
      if (surface.current) surface.current.innerHTML = clean;
      setSource(null);
      onChange(clean);
    } catch (e) {
      onError(errorText(e));
    }
  };

  const focused = typeof document !== "undefined" && document.activeElement === surface.current;
  const state = (command: string) => focused && document.queryCommandState(command);
  const visual = source === null;

  return (
    <div className="sig-editor">
      <div
        className="sig-toolbar"
        // I pulsanti non devono portare via la selezione dal testo della firma.
        onMouseDown={(e) => {
          if ((e.target as HTMLElement).closest("button")) e.preventDefault();
        }}
      >
        <select
          className="sig-select"
          value=""
          aria-label="Carattere"
          disabled={!visual}
          onChange={(e) => exec("fontName", e.target.value)}
        >
          <option value="" hidden>
            Carattere
          </option>
          {FONTS.map((f) => (
            <option key={f} value={f} style={{ fontFamily: f }}>
              {f}
            </option>
          ))}
        </select>
        <select
          className="sig-select"
          value=""
          aria-label="Dimensione"
          disabled={!visual}
          onChange={(e) => setSize(Number(e.target.value))}
        >
          <option value="" hidden>
            Dimensione
          </option>
          {SIZES.map(([label, pt]) => (
            <option key={pt} value={pt}>
              {label}
            </option>
          ))}
        </select>
        <span className="sig-sep" />
        <Tool label="Grassetto" active={state("bold")} disabled={!visual} onClick={() => exec("bold")}>
          <Bold />
        </Tool>
        <Tool label="Corsivo" active={state("italic")} disabled={!visual} onClick={() => exec("italic")}>
          <Italic />
        </Tool>
        <Tool label="Sottolineato" active={state("underline")} disabled={!visual} onClick={() => exec("underline")}>
          <Underline />
        </Tool>
        <label className={`btn btn-ghost icon-btn sig-tool sig-color ${visual ? "" : "disabled"}`} title="Colore del testo">
          <Baseline />
          <input ref={colorInput} type="color" defaultValue="#c00000" disabled={!visual} aria-label="Colore del testo" />
        </label>
        <span className="sig-sep" />
        <Tool label="Allinea a sinistra" disabled={!visual} onClick={() => exec("justifyLeft")}>
          <TextAlignStart />
        </Tool>
        <Tool label="Centra" disabled={!visual} onClick={() => exec("justifyCenter")}>
          <TextAlignCenter />
        </Tool>
        <Tool label="Allinea a destra" disabled={!visual} onClick={() => exec("justifyRight")}>
          <TextAlignEnd />
        </Tool>
        <span className="sig-sep" />
        <Tool
          label="Link"
          active={link !== null || (focused && currentLink() !== null)}
          disabled={!visual}
          onClick={() => setLink(currentLink()?.getAttribute("href") ?? "")}
        >
          <Link />
        </Tool>
        <Tool label="Immagine" disabled={!visual} onClick={() => void pickImage()}>
          <ImagePlus />
        </Tool>
        <Tool label="Colonne: logo e recapiti" disabled={!visual} onClick={() => exec("insertHTML", COLUMNS_HTML)}>
          <Columns2 />
        </Tool>
        <Tool label="Linea" disabled={!visual} onClick={() => exec("insertHorizontalRule")}>
          <SeparatorHorizontal />
        </Tool>
        <Tool label="Togli la formattazione" disabled={!visual} onClick={() => exec("removeFormat")}>
          <Eraser />
        </Tool>
        <span className="grow" />
        <Tool label={visual ? "Modifica l'HTML" : "Torna all'editor"} active={!visual} onClick={() => void toggleSource()}>
          <CodeXml />
        </Tool>
        <Help>
          Scrivi la firma come in un documento. Per un logo usa Immagine oppure incollalo con Ctrl+V; con un clic
          sull'immagine ne cambi la larghezza o la colleghi a un sito con Link. Colonne prepara lo schema logo a sinistra
          e recapiti a destra. HTML mostra il codice, per chi vuole scriverlo a mano.
        </Help>
      </div>

      {link !== null && (
        <div className="sig-bar">
          <Link />
          <input
            className="input"
            autoFocus
            value={link}
            placeholder="www.sito.it, indirizzo email o numero di telefono"
            onChange={(e) => setLink(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                applyLink();
              } else if (e.key === "Escape") {
                e.preventDefault();
                e.stopPropagation();
                setLink(null);
              }
            }}
          />
          <button className="btn btn-small btn-primary" onClick={applyLink}>
            <Check />
            Collega
          </button>
          {currentLink() && (
            <button className="btn btn-small" onClick={removeLink}>
              <Unlink />
              Togli il link
            </button>
          )}
        </div>
      )}

      {picked && visual && (
        <div className="sig-bar">
          <span className="sig-bar-label">Larghezza</span>
          <input
            type="range"
            min={16}
            max={Math.max(600, picked.naturalWidth)}
            value={picked.width}
            onChange={(e) => resize(picked, Number(e.target.value))}
            aria-label="Larghezza dell'immagine"
          />
          <input
            className="input sig-width"
            type="number"
            min={8}
            value={widthText}
            onChange={(e) => setWidthText(e.target.value)}
            onBlur={applyWidth}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                applyWidth();
              }
            }}
            aria-label="Larghezza in pixel"
          />
          <span className="sig-bar-label">px</span>
          <span className="grow" />
          <button className="btn btn-small" onClick={() => setLink(currentLink()?.getAttribute("href") ?? "")}>
            <Link />
            Collega a un sito
          </button>
          <button className="btn btn-small btn-danger" onClick={removePicked}>
            <Trash />
            Togli l'immagine
          </button>
        </div>
      )}

      <div
        ref={surface}
        className="sig-html sig-surface"
        style={visual ? undefined : { display: "none" }}
        contentEditable
        suppressContentEditableWarning
        spellCheck
        lang="it"
        data-placeholder="Nome, ruolo, telefono, sito… e un logo, se vuoi"
        onInput={emit}
        onClick={onClick}
        onPaste={onPaste}
      />
      {!visual && (
        <textarea
          className="sig-source"
          value={source ?? ""}
          spellCheck={false}
          onChange={(e) => {
            setSource(e.target.value);
            onChange(e.target.value);
          }}
        />
      )}
    </div>
  );
}

function Tool({
  label,
  active,
  disabled,
  onClick,
  children,
}: {
  label: string;
  active?: boolean;
  disabled?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      className="btn btn-ghost icon-btn sig-tool"
      title={label}
      aria-label={label}
      aria-pressed={active ?? false}
      disabled={disabled}
      onClick={onClick}
    >
      {children}
    </button>
  );
}

/** "www.sito.it" → https://, un indirizzo → mailto:, un numero → tel:. */
function normalizeUrl(input: string): string {
  const v = input.trim();
  if (!v) return "";
  if (/^(https?:|mailto:|tel:)/i.test(v)) return v;
  if (/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(v)) return `mailto:${v}`;
  if (/^\+?[\d\s./-]{6,}$/.test(v)) return `tel:${v.replace(/[\s./-]/g, "")}`;
  return `https://${v}`;
}

function escapeHtml(text: string): string {
  return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

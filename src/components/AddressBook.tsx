import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  ArrowDownAZ,
  AtSign,
  BookUser,
  Building2,
  CalendarPlus,
  ExternalLink,
  Globe,
  Import,
  LoaderCircle,
  Mail,
  MapPin,
  Pencil,
  Phone,
  Receipt,
  Search,
  SquarePen,
  StickyNote,
  Trash,
  UserPlus,
} from "lucide-react";
import type { Account, Company, CompanyInput, Contact, ContactInput } from "../api";
import {
  alphabeticalSections,
  companyIndex,
  companyKey,
  contactLabel,
  domainSections,
  recipient,
  type CompanyInfo,
} from "../book";
import { avatarHue, initials } from "../format";
import { CompanyCard } from "./CompanyCard";
import { ContactCard } from "./ContactCard";
import { OutlookImport } from "./OutlookImport";

type Grouping = "alpha" | "domain";
const GROUPING_KEY = "postamon.rubrica-gruppi";

function savedGrouping(): Grouping {
  try {
    return localStorage.getItem(GROUPING_KEY) === "domain" ? "domain" : "alpha";
  } catch {
    return "alpha";
  }
}

interface Props {
  account: Account;
  /** `null` finché la rubrica non è caricata. */
  contacts: Contact[] | null;
  companies: Company[];
  selectedId: number | null;
  onSelect: (id: number | null) => void;
  onSave: (contact: ContactInput) => Promise<Contact>;
  onDelete: (contact: Contact) => Promise<void>;
  onSaveCompany: (company: CompanyInput) => Promise<Company>;
  /** Importa contatti (da Outlook) senza sovrascrivere quelli che ci sono. */
  onImport: (list: ContactInput[]) => Promise<void>;
  /** Nuovo messaggio a questi destinatari. */
  onWrite: (to: string) => void;
  suggestCompany: (address: string) => Promise<string | null>;
}

const EMPTY: ContactInput = { address: "", name: "", company: "", phone: "", notes: "" };

function toInput({ id, address, name, company, phone, notes }: Contact): ContactInput {
  return { id, address, name, company, phone, notes };
}

function companyInput(info: CompanyInfo): CompanyInput {
  const card = info.card;
  return {
    id: card?.id,
    previousName: info.name,
    name: info.name,
    vat: card?.vat ?? "",
    phone: card?.phone ?? "",
    address: card?.address ?? "",
    website: card?.website ?? "",
    notes: card?.notes ?? "",
  };
}

/** Colore dell'azienda: dal suo primo dominio, così non cambia se la si rinomina. */
const companyHue = (info: CompanyInfo | undefined) => avatarHue(info?.domains[0] ?? info?.key ?? "");

/** Rubrica della casella: elenco a sinistra, scheda della persona o dell'azienda a destra. */
export function AddressBook({
  account,
  contacts,
  companies,
  selectedId,
  onSelect,
  onSave,
  onDelete,
  onSaveCompany,
  onImport,
  onWrite,
  suggestCompany,
}: Props) {
  const [query, setQuery] = useState("");
  const [grouping, setGrouping] = useState<Grouping>(savedGrouping);
  // Azienda aperta a destra al posto di una persona.
  const [companySel, setCompanySel] = useState<string | null>(null);
  const [card, setCard] = useState<{ anchor: HTMLElement; contact: Contact | null } | null>(null);
  const [companyCard, setCompanyCard] = useState<{ anchor: HTMLElement; info: CompanyInfo } | null>(null);
  // Pulsante da cui si è aperta l'importazione da Outlook.
  const [importFrom, setImportFrom] = useState<HTMLElement | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const list = useRef<HTMLDivElement>(null);

  useEffect(() => {
    setQuery("");
    setCard(null);
    setCompanyCard(null);
    setCompanySel(null);
    setImportFrom(null);
  }, [account.id]);

  useEffect(() => {
    setConfirmDelete(false);
    if (selectedId !== null) setCompanySel(null);
  }, [selectedId]);

  const chooseGrouping = (g: Grouping) => {
    setGrouping(g);
    try {
      localStorage.setItem(GROUPING_KEY, g);
    } catch {
      // preferenza solo per questa sessione
    }
  };

  const index = useMemo(() => companyIndex(contacts ?? [], companies), [contacts, companies]);
  const companyNames = useMemo(
    () => [...index.companies.values()].map((c) => c.name).sort((a, b) => a.localeCompare(b, "it")),
    [index],
  );

  const sections = useMemo(() => {
    const q = query.trim().toLowerCase();
    const shown = (contacts ?? []).filter(
      (c) => !q || [c.name, c.address, c.company, c.phone].some((v) => v.toLowerCase().includes(q)),
    );
    return grouping === "domain" ? domainSections(shown, index) : alphabeticalSections(shown);
  }, [contacts, query, grouping, index]);

  const visible = sections.flatMap((s) => (s.kind === "company" ? s.groups.flatMap((g) => g.items) : s.items));
  const selected = companySel ? null : (contacts?.find((c) => c.id === selectedId) ?? null);
  const company = companySel ? (index.companies.get(companySel) ?? null) : null;

  const openCompany = (key: string) => {
    onSelect(null);
    setCompanySel(key);
  };

  const pickContact = (id: number) => {
    setCompanySel(null);
    onSelect(id);
  };

  // La riga scelta resta visibile quando si scorre con la tastiera.
  useEffect(() => {
    list.current?.querySelector(".contact-row.selected, .book-company-head.selected")?.scrollIntoView({
      block: "nearest",
    });
  }, [selectedId, companySel, grouping]);

  const onKey = (e: KeyboardEvent) => {
    if (!visible.length || (e.key !== "ArrowDown" && e.key !== "ArrowUp")) return;
    e.preventDefault();
    const idx = visible.findIndex((c) => c.id === selectedId);
    const next = e.key === "ArrowDown" ? Math.min(visible.length - 1, idx + 1) : Math.max(0, idx - 1);
    pickContact(visible[next].id);
  };

  const remove = async (c: Contact) => {
    setDeleting(true);
    try {
      await onDelete(c);
    } finally {
      setDeleting(false);
      setConfirmDelete(false);
    }
  };

  const count = contacts?.length ?? 0;

  const row = (c: Contact) => (
    <button
      key={c.id}
      className={`contact-row ${!companySel && c.id === selectedId ? "selected" : ""}`}
      onClick={() => pickContact(c.id)}
    >
      <span className="contact-avatar" style={{ background: `hsl(${avatarHue(c.address)} 42% 42%)` }}>
        {initials(c.name, c.address)}
      </span>
      <span className="contact-row-text">
        <span className="contact-row-name">{contactLabel(c)}</span>
        <span className="contact-row-meta">{c.company ? `${c.company} · ${c.address}` : c.address}</span>
      </span>
    </button>
  );

  const companyLink = (key: string) => {
    const info = index.companies.get(key);
    if (!info) return null;
    return (
      <button
        className={`book-company-link ${companySel === key ? "selected" : ""}`}
        onClick={() => openCompany(key)}
        title="Apri la scheda dell'azienda"
      >
        <Building2 />
        {info.name}
      </button>
    );
  };

  return (
    <>
      <section className="sheet">
        <div className="list-head">
          <div className="list-title">
            <h2>Rubrica</h2>
            <span className="sub">
              {account.email} · {count === 1 ? "1 contatto" : `${count} contatti`}
            </span>
            <button
              className="btn btn-small book-import"
              onClick={(e) => setImportFrom(e.currentTarget)}
              title="Importa i contatti dalla rubrica di Outlook"
            >
              <Import />
              Importa
            </button>
            <button
              className="btn btn-primary btn-small"
              onClick={(e) => setCard({ anchor: e.currentTarget, contact: null })}
            >
              <UserPlus />
              Nuovo contatto
            </button>
          </div>
          <label className="search">
            <Search />
            <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Cerca nella rubrica" />
          </label>
          <div className="segmented" role="group" aria-label="Come mostrare i contatti">
            <button aria-pressed={grouping === "alpha"} onClick={() => chooseGrouping("alpha")}>
              <ArrowDownAZ />
              Ordine alfabetico
            </button>
            <button aria-pressed={grouping === "domain"} onClick={() => chooseGrouping("domain")}>
              <AtSign />
              Per dominio
            </button>
          </div>
        </div>
        <div className="list" ref={list} tabIndex={0} onKeyDown={onKey}>
          {contacts === null && (
            <div className="empty">
              <LoaderCircle className="spin" />
            </div>
          )}
          {contacts !== null && visible.length === 0 && (
            <div className="empty">
              <div>
                <BookUser />
                <div>{query ? "Nessun contatto trovato" : "La rubrica è vuota"}</div>
                {!query && (
                  <button className="btn btn-small book-empty-import" onClick={(e) => setImportFrom(e.currentTarget)}>
                    <Import />
                    Importa da Outlook
                  </button>
                )}
              </div>
            </div>
          )}
          {sections.map((s) =>
            s.kind === "company" ? (
              <div key={s.key} className="book-company">
                <button
                  className={`book-company-head ${companySel === s.company ? "selected" : ""}`}
                  onClick={() => openCompany(s.company)}
                  title="Apri la scheda dell'azienda"
                >
                  <span
                    className="company-badge"
                    style={{ background: `hsl(${companyHue(index.companies.get(s.company))} 38% 40%)` }}
                  >
                    <Building2 />
                  </span>
                  <span className="book-company-name">{index.companies.get(s.company)?.name}</span>
                  <span className="book-group-count">{s.count}</span>
                </button>
                {s.groups.map((g) => (
                  <div key={g.key} className="book-subgroup">
                    <div className="book-group-head">
                      <span className="book-group-name">{g.domain ? `@${g.domain}` : "Altri indirizzi"}</span>
                      <span className="book-group-count">{g.items.length}</span>
                    </div>
                    {g.items.map(row)}
                  </div>
                ))}
              </div>
            ) : (
              <div key={s.key} className="book-group">
                <div className="book-group-head">
                  <span className="book-group-name">{s.kind === "domain" ? `@${s.domain}` : s.letter}</span>
                  {s.kind === "domain" && s.company && companyLink(s.company)}
                  {s.kind === "domain" && <span className="book-group-count">{s.items.length}</span>}
                </div>
                {s.items.map(row)}
              </div>
            ),
          )}
        </div>
      </section>

      <section className="sheet">
        {company ? (
          <CompanyPane
            info={company}
            onEdit={(anchor) => setCompanyCard({ anchor, info: company })}
            onPick={pickContact}
            onWrite={onWrite}
          />
        ) : selected ? (
          <div className="contact-view">
            <div className="contact-head selectable">
              <div className="avatar avatar-big" style={{ background: `hsl(${avatarHue(selected.address)} 42% 42%)` }}>
                {initials(selected.name, selected.address)}
              </div>
              <div className="contact-head-main">
                <h1>{contactLabel(selected)}</h1>
                <div className="sender-addr">{selected.address}</div>
              </div>
            </div>
            <div className="toolbar">
              <button className="btn btn-primary" onClick={() => onWrite(recipient(selected))}>
                <SquarePen />
                Scrivi
              </button>
              <button className="btn" onClick={(e) => setCard({ anchor: e.currentTarget, contact: selected })}>
                <Pencil />
                Modifica
              </button>
              <span className="spacer" />
              {confirmDelete ? (
                <button className="btn btn-danger" onClick={() => void remove(selected)} disabled={deleting}>
                  {deleting ? <LoaderCircle className="spin" /> : <Trash />}
                  Conferma: togli il contatto
                </button>
              ) : (
                <button className="btn btn-danger" onClick={() => setConfirmDelete(true)}>
                  <Trash />
                  Togli dalla rubrica
                </button>
              )}
            </div>
            <dl className="contact-details selectable">
              <dt>
                <Mail />
                Email
              </dt>
              <dd>{selected.address}</dd>
              <dt>
                <Phone />
                Telefono
              </dt>
              <dd className={selected.phone ? "" : "none"}>{selected.phone || "Non indicato"}</dd>
              <dt>
                <Building2 />
                Azienda
              </dt>
              <dd className={selected.company ? "" : "none"}>
                {selected.company ? (
                  <button
                    className="in-book"
                    onClick={() => openCompany(companyKey(selected.company))}
                    title="Apri la scheda dell'azienda"
                  >
                    <Building2 />
                    {selected.company}
                  </button>
                ) : (
                  "Non indicata"
                )}
              </dd>
              <dt>
                <StickyNote />
                Note
              </dt>
              <dd className={`contact-notes ${selected.notes ? "" : "none"}`}>{selected.notes || "Nessuna nota"}</dd>
              {selected.created > 0 && (
                <>
                  <dt>
                    <CalendarPlus />
                    In rubrica dal
                  </dt>
                  <dd>
                    {new Date(selected.created * 1000).toLocaleDateString("it-IT", {
                      day: "numeric",
                      month: "long",
                      year: "numeric",
                    })}
                  </dd>
                </>
              )}
            </dl>
          </div>
        ) : (
          <div className="empty">
            <div>
              <BookUser />
              <div>Seleziona un contatto</div>
            </div>
          </div>
        )}
      </section>

      {card && (
        <ContactCard
          anchor={card.anchor}
          title={card.contact ? "Modifica contatto" : "Nuovo contatto"}
          initial={card.contact ? toInput(card.contact) : EMPTY}
          onSave={async (input) => {
            const saved = await onSave(input);
            pickContact(saved.id);
          }}
          onClose={() => setCard(null)}
          suggestCompany={suggestCompany}
          companyNames={companyNames}
        />
      )}
      {companyCard && (
        <CompanyCard
          anchor={companyCard.anchor}
          initial={companyInput(companyCard.info)}
          onSave={async (input) => {
            const saved = await onSaveCompany(input);
            setCompanySel(companyKey(saved.name));
          }}
          onClose={() => setCompanyCard(null)}
        />
      )}
      {importFrom && (
        <OutlookImport
          anchor={importFrom}
          email={account.email}
          contacts={contacts ?? []}
          onImport={onImport}
          onClose={() => setImportFrom(null)}
        />
      )}
    </>
  );
}

/** Indirizzo del sito, con https:// se manca. */
function siteUrl(website: string): string {
  return /^https?:\/\//i.test(website) ? website : `https://${website}`;
}

function CompanyPane({
  info,
  onEdit,
  onPick,
  onWrite,
}: {
  info: CompanyInfo;
  onEdit: (anchor: HTMLElement) => void;
  onPick: (id: number) => void;
  onWrite: (to: string) => void;
}) {
  const card = info.card;
  const field = (value: string | undefined, empty: string) => (
    <dd className={value ? "" : "none"}>{value || empty}</dd>
  );
  return (
    <div className="contact-view">
      <div className="contact-head selectable">
        <div className="avatar avatar-big company-avatar" style={{ background: `hsl(${companyHue(info)} 38% 40%)` }}>
          <Building2 />
        </div>
        <div className="contact-head-main">
          <h1>{info.name}</h1>
          <div className="company-domains">
            {info.domains.length > 0 ? (
              info.domains.map((d) => (
                <span key={d} className="domain-chip">
                  @{d}
                </span>
              ))
            ) : (
              <span className="sender-addr">Nessun dominio aziendale</span>
            )}
          </div>
        </div>
      </div>
      <div className="toolbar">
        <button
          className="btn btn-primary"
          disabled={info.people.length === 0}
          onClick={() => onWrite(info.people.map(recipient).join(", "))}
        >
          <SquarePen />
          Scrivi a tutti
        </button>
        <button className="btn" onClick={(e) => onEdit(e.currentTarget)}>
          <Pencil />
          Modifica
        </button>
      </div>
      <dl className="contact-details selectable">
        <dt>
          <Receipt />
          Partita IVA
        </dt>
        {field(card?.vat, "Non indicata")}
        <dt>
          <Phone />
          Telefono
        </dt>
        {field(card?.phone, "Non indicato")}
        <dt>
          <MapPin />
          Indirizzo
        </dt>
        <dd className={`contact-notes ${card?.address ? "" : "none"}`}>{card?.address || "Non indicato"}</dd>
        <dt>
          <Globe />
          Sito web
        </dt>
        {card?.website ? (
          <dd className="company-site">
            <span>{card.website}</span>
            <button
              className="btn btn-ghost icon-btn"
              onClick={() => void openUrl(siteUrl(card.website))}
              title="Apri il sito nel browser"
              aria-label="Apri il sito nel browser"
            >
              <ExternalLink />
            </button>
          </dd>
        ) : (
          <dd className="none">Non indicato</dd>
        )}
        <dt>
          <StickyNote />
          Note
        </dt>
        <dd className={`contact-notes ${card?.notes ? "" : "none"}`}>{card?.notes || "Nessuna nota"}</dd>
      </dl>
      <div className="company-people">
        <div className="book-group-head">
          <span className="book-group-name">Persone</span>
          <span className="book-group-count">{info.people.length}</span>
        </div>
        {info.people.map((c) => (
          <button key={c.id} className="contact-row" onClick={() => onPick(c.id)}>
            <span className="contact-avatar" style={{ background: `hsl(${avatarHue(c.address)} 42% 42%)` }}>
              {initials(c.name, c.address)}
            </span>
            <span className="contact-row-text">
              <span className="contact-row-name">{contactLabel(c)}</span>
              <span className="contact-row-meta">{c.address}</span>
            </span>
          </button>
        ))}
      </div>
    </div>
  );
}

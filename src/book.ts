// Rubrica: nomi da mostrare e gruppi della vista "Per dominio", dove le
// aziende raccolgono i loro domini e le persone che scrivono da Gmail & co.

import type { Company, Contact } from "./api";

/** Nome da mostrare: quello scritto in rubrica, altrimenti l'indirizzo. */
export function contactLabel(c: Contact): string {
  return c.name || c.address;
}

/** Destinatario per un nuovo messaggio: "Nome <indirizzo>". */
export function recipient(c: Contact): string {
  return c.name ? `${c.name} <${c.address}>` : c.address;
}

export function domainOf(address: string): string {
  return address.slice(address.lastIndexOf("@") + 1);
}

/** Le aziende si riconoscono dal nome, senza badare a maiuscole e spazi. */
export function companyKey(name: string): string {
  return name.trim().toLocaleLowerCase("it");
}

export const byLabel = (a: Contact, b: Contact) =>
  contactLabel(a).localeCompare(contactLabel(b), "it", { sensitivity: "base" });

const byText = (a: string, b: string) => a.localeCompare(b, "it", { sensitivity: "base" });

export interface CompanyInfo {
  key: string;
  name: string;
  /** Scheda con i dati dell'azienda, se è già stata compilata. */
  card: Company | null;
  /** Domini aziendali dove questa è l'azienda della maggior parte delle persone. */
  domains: string[];
  people: Contact[];
}

/**
 * Aziende della rubrica: quelle scritte nel campo "Azienda" delle persone e
 * quelle con una scheda. Restituisce anche, per ogni dominio aziendale,
 * l'azienda a cui appartiene.
 */
export function companyIndex(contacts: Contact[], cards: Company[]) {
  const companies = new Map<string, CompanyInfo>();
  const spellings = new Map<string, Map<string, number>>();
  const add = (key: string, name: string): CompanyInfo => {
    let info = companies.get(key);
    if (!info) {
      info = { key, name, card: null, domains: [], people: [] };
      companies.set(key, info);
    }
    return info;
  };
  for (const card of cards) add(companyKey(card.name), card.name).card = card;
  // Conteggio delle aziende per dominio aziendale.
  const perDomain = new Map<string, Map<string, number>>();
  for (const c of contacts) {
    const key = companyKey(c.company);
    if (!key) continue;
    add(key, c.company.trim()).people.push(c);
    const spelled = spellings.get(key) ?? new Map<string, number>();
    spelled.set(c.company.trim(), (spelled.get(c.company.trim()) ?? 0) + 1);
    spellings.set(key, spelled);
    if (c.business) {
      const domain = domainOf(c.address);
      const counts = perDomain.get(domain) ?? new Map<string, number>();
      counts.set(key, (counts.get(key) ?? 0) + 1);
      perDomain.set(domain, counts);
    }
  }
  // Senza scheda il nome è quello scritto più spesso.
  for (const info of companies.values()) {
    if (info.card) continue;
    const spelled = [...(spellings.get(info.key) ?? [])].sort((a, b) => b[1] - a[1] || byText(a[0], b[0]));
    if (spelled.length) info.name = spelled[0][0];
  }
  const owner = new Map<string, string>();
  for (const [domain, counts] of perDomain) {
    const [key] = [...counts].sort((a, b) => b[1] - a[1] || byText(a[0], b[0]))[0];
    owner.set(domain, key);
    companies.get(key)!.domains.push(domain);
  }
  for (const info of companies.values()) {
    info.domains.sort(byText);
    info.people.sort(byLabel);
  }
  return { companies, owner };
}

export interface DomainGroup {
  key: string;
  /** `null` per le persone dell'azienda che scrivono da un dominio comune. */
  domain: string | null;
  items: Contact[];
}

export type Section =
  | { kind: "letter"; key: string; letter: string; items: Contact[] }
  /** Un dominio; `company` è l'azienda che lo possiede, se ce l'ha. */
  | { kind: "domain"; key: string; domain: string; company: string | null; items: Contact[] }
  /** Un'azienda con più domini: un sottogruppo per dominio. */
  | { kind: "company"; key: string; company: string; groups: DomainGroup[]; count: number };

function letterOf(label: string): string {
  const first = label.normalize("NFD").replace(/\p{M}/gu, "").charAt(0).toUpperCase();
  return /[A-Z]/.test(first) ? first : "#";
}

export function alphabeticalSections(shown: Contact[]): Section[] {
  const map = new Map<string, Contact[]>();
  for (const c of [...shown].sort(byLabel)) {
    const letter = letterOf(contactLabel(c));
    map.set(letter, [...(map.get(letter) ?? []), c]);
  }
  // "#" (nomi che non iniziano con una lettera) va in fondo.
  const letters = [...map.keys()].sort((a, b) => (a === "#" || b === "#" ? (a === "#" ? 1 : -1) : byText(a, b)));
  return letters.map((letter): Section => ({ kind: "letter", key: letter, letter, items: map.get(letter)! }));
}

/**
 * Vista "Per dominio". Chi ha un dominio aziendale resta nel suo dominio.
 * Chi scrive da un dominio comune ma ha l'azienda di un dominio aziendale va
 * nel gruppo di quel dominio; se l'azienda ha più domini, i domini diventano
 * sottogruppi dell'azienda e quelle persone vanno in "Altri indirizzi".
 */
export function domainSections(
  shown: Contact[],
  { companies, owner }: ReturnType<typeof companyIndex>,
): Section[] {
  const byDomain = new Map<string, Contact[]>();
  const others = new Map<string, Contact[]>();
  const push = (map: Map<string, Contact[]>, key: string, c: Contact) => map.set(key, [...(map.get(key) ?? []), c]);
  for (const c of shown) {
    const domains = companies.get(companyKey(c.company))?.domains ?? [];
    if (c.business || domains.length === 0) push(byDomain, domainOf(c.address), c);
    else if (domains.length === 1) push(byDomain, domains[0], c);
    else push(others, companyKey(c.company), c);
  }
  const sections: { label: string; section: Section }[] = [];
  const merged = new Set<string>();
  for (const info of companies.values()) {
    if (info.domains.length < 2) continue;
    const groups: DomainGroup[] = info.domains
      .map((domain) => ({ key: domain, domain, items: (byDomain.get(domain) ?? []).sort(byLabel) }))
      .filter((g) => g.items.length > 0);
    info.domains.forEach((d) => merged.add(d));
    const extra = (others.get(info.key) ?? []).sort(byLabel);
    if (extra.length) groups.push({ key: "", domain: null, items: extra });
    const count = groups.reduce((n, g) => n + g.items.length, 0);
    if (count === 0) continue;
    sections.push({
      label: info.name,
      section: { kind: "company", key: `azienda:${info.key}`, company: info.key, groups, count },
    });
  }
  for (const [domain, items] of byDomain) {
    if (merged.has(domain)) continue;
    sections.push({
      label: domain,
      section: { kind: "domain", key: domain, domain, company: owner.get(domain) ?? null, items: items.sort(byLabel) },
    });
  }
  return sections.sort((a, b) => byText(a.label, b.label)).map((s) => s.section);
}

// Formattazione di date, dimensioni e nomi per l'interfaccia.

const DAY = 24 * 60 * 60 * 1000;

function startOfDay(d: Date): number {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

/** Data compatta per l'elenco: "10:42", "ieri", "lun", "21 ott", "21/10/25". */
export function listDate(seconds: number): string {
  if (!seconds) return "";
  const d = new Date(seconds * 1000);
  const now = new Date();
  const diffDays = Math.round((startOfDay(now) - startOfDay(d)) / DAY);
  if (diffDays <= 0) return d.toLocaleTimeString("it-IT", { hour: "2-digit", minute: "2-digit" });
  if (diffDays === 1) return "ieri";
  if (diffDays < 7) return d.toLocaleDateString("it-IT", { weekday: "short" });
  if (d.getFullYear() === now.getFullYear()) return d.toLocaleDateString("it-IT", { day: "numeric", month: "short" });
  return d.toLocaleDateString("it-IT", { day: "2-digit", month: "2-digit", year: "2-digit" });
}

/** Data completa per il messaggio aperto: "mercoledì 21 ottobre 2026, 10:42". */
export function fullDate(seconds: number): string {
  if (!seconds) return "";
  const d = new Date(seconds * 1000);
  const day = d.toLocaleDateString("it-IT", { weekday: "long", day: "numeric", month: "long", year: "numeric" });
  const time = d.toLocaleTimeString("it-IT", { hour: "2-digit", minute: "2-digit" });
  return `${day}, ${time}`;
}

export function fileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toLocaleString("it-IT", { maximumFractionDigits: 1 })} MB`;
}

export function initials(name: string, email: string): string {
  const source = name.trim() || email.split("@")[0];
  const words = source.split(/[\s._-]+/).filter(Boolean);
  const letters = words.length > 1 ? words[0][0] + words[1][0] : source.slice(0, 2);
  return letters.toUpperCase();
}

/** Colore stabile per l'iniziale del mittente. */
export function avatarHue(email: string): number {
  let h = 0;
  for (const c of email.toLowerCase()) h = (h * 31 + c.charCodeAt(0)) % 360;
  return h;
}

export function senderLabel(name: string, addr: string): string {
  return name.trim() || addr || "(sconosciuto)";
}

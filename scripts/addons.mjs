// Controllo e pacchetti degli add-on in addons/<id>/ (vedi ADDONS.md).
//
//   node scripts/addons.mjs check            controlla tutti gli add-on (lo fa la CI su ogni pull request)
//   node scripts/addons.mjs check <id>       solo quello
//   node scripts/addons.mjs pack <cartella>  controlla, poi scrive <id>-<versione>.zip e addons.json
//
// Le regole sono le stesse che Postamon applica installando (src-tauri/src/addons.rs):
// se qui passa, il pacchetto si installa. In più si cercano le cose che la
// revisione rifiuta comunque (codice minificato, WebRTC, eval, script in linea),
// così chi scrive l'add-on lo sa prima di aprire la pull request.
//
// Nessuna dipendenza: solo Node.

import { createHash } from "node:crypto";
import { existsSync, lstatSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
// La CI usa la copia di questo script presa da main, così una pull request
// non può allentare le regole con cui viene controllata: la cartella degli
// add-on da controllare arriva da qui.
const ADDONS = process.env.POSTAMON_ADDONS_DIR ?? join(ROOT, "addons");

const API_VERSION = 1;
const PERMISSIONS = ["message.read", "compose"];
const HOOKS = ["message-button", "sidebar-view"];
const FILE_TYPES = ["html", "js", "mjs", "css", "json", "svg", "png", "jpg", "jpeg", "gif", "webp", "ico", "woff", "woff2", "ttf", "otf", "txt", "md"];
const MAX_PACKAGE = 5 * 1024 * 1024;
const MAX_FILES = 300;
const MAX_HOSTS = 5;
const MAX_LINE = 400;
const SEMVER = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$/;

// ---- regole di addon.json (come Manifest::validate) ----

const validId = (id) => typeof id === "string" && /^[a-z0-9][a-z0-9-]{0,38}[a-z0-9]$/.test(id);

function validHost(host) {
  const labels = host.split(".");
  return (
    host.length <= 253 &&
    labels.length >= 2 &&
    labels.every((l) => /^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$/.test(l)) &&
    !/^\d+$/.test(labels[labels.length - 1])
  );
}

function packagePath(path) {
  if (typeof path !== "string" || path === "" || path.length > 200 || path.startsWith("/")) return "percorso non ammesso";
  if (!/^[A-Za-z0-9._\-/]+$/.test(path)) return "percorso non ammesso (solo lettere, cifre, . _ - /)";
  if (path.split("/").some((p) => p === "" || p === ".." || p.startsWith("."))) return "percorso non ammesso";
  if (path === "_postamon" || path.startsWith("_postamon/")) return "_postamon/ è riservata a Postamon";
  const ext = path.includes(".") ? path.slice(path.lastIndexOf(".") + 1).toLowerCase() : "";
  if (!FILE_TYPES.includes(ext)) return `tipo di file non ammesso (.${ext || "senza estensione"})`;
  return null;
}

function checkText(errors, field, value, max) {
  if (typeof value !== "string" || value.trim() === "") errors.push(`${field} è vuoto`);
  else if ([...value].length > max) errors.push(`${field} supera i ${max} caratteri`);
  else if (/[\u0000-\u001f\u007f]/.test(value)) errors.push(`${field} contiene caratteri di controllo`);
}

function checkManifest(m, errors) {
  if (!validId(m.id)) errors.push(`id «${m.id}» non valido: da 2 a 40 caratteri fra a-z, 0-9 e trattino`);
  checkText(errors, "name", m.name, 60);
  checkText(errors, "description", m.description, 300);
  checkText(errors, "author", m.author, 80);
  if (typeof m.version !== "string" || !SEMVER.test(m.version)) errors.push(`version «${m.version}» non è nel formato 1.2.3`);
  if (!Number.isInteger(m.api) || m.api < 1) errors.push("api deve essere un numero intero, almeno 1");
  else if (m.api > API_VERSION) errors.push(`api ${m.api}: Postamon oggi supporta fino alla ${API_VERSION}`);
  if (m.minPostamon !== undefined && (typeof m.minPostamon !== "string" || !SEMVER.test(m.minPostamon)))
    errors.push(`minPostamon «${m.minPostamon}» non è nel formato 1.2.3`);
  if (m.icon !== undefined) {
    const why = packagePath(m.icon);
    if (why) errors.push(`icon: ${why}`);
    else if (!/\.(svg|png)$/.test(m.icon)) errors.push("icon deve essere un file .svg o .png");
  }
  const permissions = m.permissions ?? [];
  if (!Array.isArray(permissions)) errors.push("permissions deve essere un elenco");
  else {
    let hosts = 0;
    permissions.forEach((p, i) => {
      if (typeof p !== "string") return errors.push("permissions contiene un valore che non è testo");
      if (p.startsWith("network:")) {
        if (!validHost(p.slice(8))) errors.push(`permesso «${p}»: serve un nome di dominio come api.esempio.it`);
        hosts++;
      } else if (!PERMISSIONS.includes(p)) errors.push(`permesso sconosciuto: «${p}»`);
      if (permissions.indexOf(p) < i) errors.push(`permesso ripetuto: «${p}»`);
    });
    if (hosts > MAX_HOSTS) errors.push(`al massimo ${MAX_HOSTS} domini nei permessi network:`);
  }
  if (!Array.isArray(m.hooks) || m.hooks.length < 1 || m.hooks.length > 8) errors.push("hooks deve avere da 1 a 8 punti di aggancio");
  else
    for (const h of m.hooks) {
      if (!HOOKS.includes(h?.type)) errors.push(`punto di aggancio sconosciuto: «${h?.type}»`);
      checkText(errors, "title di un punto di aggancio", h?.title, 40);
      const why = packagePath(h?.page);
      if (why) errors.push(`page «${h?.page}»: ${why}`);
      else if (!h.page.endsWith(".html")) errors.push(`page «${h.page}»: deve essere un file .html`);
    }
}

// ---- file del pacchetto ----

function listFiles(dir, errors) {
  const out = [];
  const walk = (d) => {
    for (const name of readdirSync(d).sort()) {
      const full = join(d, name);
      const rel = relative(dir, full).split(sep).join("/");
      const st = lstatSync(full);
      if (st.isSymbolicLink()) errors.push(`${rel}: i collegamenti simbolici non sono ammessi`);
      else if (st.isDirectory()) walk(full);
      else {
        const why = packagePath(rel);
        if (why) errors.push(`${rel}: ${why}`);
        out.push({ rel, full, size: st.size });
      }
    }
  };
  walk(dir);
  return out;
}

/** Cose che Postamon blocca o che la revisione rifiuta: meglio saperle subito. */
function checkCode(file, text, hosts, errors, notes) {
  const lines = text.split(/\r?\n/);
  const long = lines.findIndex((l) => l.length > MAX_LINE);
  if (/\.(m?js|html|css)$/.test(file) && long >= 0)
    errors.push(`${file}:${long + 1}: riga di oltre ${MAX_LINE} caratteri. Sembra codice minificato o generato: nel repo va il sorgente leggibile`);
  if (/\.(m?js|html)$/.test(file)) {
    const banned = [
      [/\b(webkit)?RTC(PeerConnection|DataChannel)\b/, "WebRTC non è ammesso"],
      [/\beval\s*\(|\bnew\s+Function\s*\(/, "eval e new Function non sono ammessi (Postamon li blocca)"],
      [/\bdocument\.write(ln)?\s*\(/, "document.write non è ammesso"],
      [/\bimportScripts\s*\(\s*["'`]https?:/, "script da internet non ammessi"],
    ];
    lines.forEach((line, i) => {
      for (const [re, why] of banned) if (re.test(line)) errors.push(`${file}:${i + 1}: ${why}`);
    });
    for (const m of text.matchAll(/["'`](https?):\/\/([a-z0-9.-]+)/gi)) {
      const host = m[2].toLowerCase();
      if (m[1].toLowerCase() === "http") notes.push(`${file}: indirizzo http://${host} (non cifrato): Postamon accetta solo https`);
      else if (!hosts.includes(host)) notes.push(`${file}: collegamento a ${host} senza il permesso network:${host}: Postamon lo bloccherà`);
    }
  }
  if (file.endsWith(".html")) {
    const lineAt = (index) => text.slice(0, index).split("\n").length;
    for (const m of text.matchAll(/<script\b(?![^>]*\bsrc\s*=)[^>]*>([\s\S]*?)<\/script>/gi))
      if (m[1].trim()) errors.push(`${file}:${lineAt(m.index)}: script in linea: Postamon lo blocca, mettilo in un file .js`);
    for (const m of text.matchAll(/<[a-z][^>]*\son[a-z]+\s*=/gi))
      errors.push(`${file}:${lineAt(m.index)}: gestore in linea (onclick=…): Postamon lo blocca, usa addEventListener in un file .js`);
    for (const m of text.matchAll(/<(?!a\s)[a-z]+[^>]*\b(src|href)\s*=\s*["']\s*(https?:)?\/\//gi))
      errors.push(`${file}:${lineAt(m.index)}: risorsa da internet: Postamon la blocca, mettila nel pacchetto`);
    if (!text.includes("_postamon/sdk.js")) notes.push(`${file}: non carica _postamon/sdk.js, quindi non può parlare con Postamon`);
  }
}

function checkAddon(id) {
  const dir = join(ADDONS, id);
  const errors = [];
  const notes = [];
  let manifest = null;
  const manifestPath = join(dir, "addon.json");
  if (!existsSync(manifestPath)) errors.push("manca addon.json");
  else {
    try {
      manifest = JSON.parse(readFileSync(manifestPath, "utf8").replace(/^﻿/, ""));
    } catch (e) {
      errors.push(`addon.json non è JSON valido: ${e.message}`);
    }
  }
  if (manifest) {
    checkManifest(manifest, errors);
    if (manifest.id !== id) errors.push(`l'id in addon.json («${manifest.id}») deve essere uguale al nome della cartella («${id}»)`);
  }
  const files = listFiles(dir, errors);
  if (files.length > MAX_FILES) errors.push(`troppi file (${files.length}, al massimo ${MAX_FILES})`);
  const total = files.reduce((n, f) => n + f.size, 0);
  if (total > MAX_PACKAGE) errors.push(`pacchetto troppo grande (${(total / 1048576).toFixed(1)} MB, al massimo 5 MB)`);
  if (manifest && Array.isArray(manifest.hooks)) {
    const names = new Set(files.map((f) => f.rel));
    for (const p of [...manifest.hooks.map((h) => h?.page), manifest.icon].filter(Boolean))
      if (typeof p === "string" && !names.has(p)) errors.push(`addon.json indica ${p}, che non c'è`);
  }
  const hosts = (manifest?.permissions ?? []).filter((p) => typeof p === "string" && p.startsWith("network:")).map((p) => p.slice(8));
  for (const f of files)
    if (/\.(m?js|html|css)$/.test(f.rel)) checkCode(f.rel, readFileSync(f.full, "utf8"), hosts, errors, notes);
  return { id, dir, manifest, files, errors, notes, hosts };
}

function addonIds(only) {
  if (!existsSync(ADDONS)) return [];
  const ids = readdirSync(ADDONS).filter((n) => lstatSync(join(ADDONS, n)).isDirectory()).sort();
  if (only) {
    if (!ids.includes(only)) throw new Error(`addons/${only} non esiste`);
    return [only];
  }
  return ids;
}

function report(results) {
  let failed = false;
  for (const r of results) {
    const name = r.manifest?.name ? `${r.id} (${r.manifest.name} ${r.manifest.version})` : r.id;
    console.log(`${r.errors.length ? "✗" : "✓"} ${name}`);
    for (const e of r.errors) console.log(`    errore: ${e}`);
    for (const n of r.notes) console.log(`    nota:   ${n}`);
    if (r.manifest && !r.errors.length) {
      const perms = r.manifest.permissions ?? [];
      console.log(`    permessi: ${perms.length ? perms.join(", ") : "nessuno"}`);
      if (r.hosts.length && perms.includes("message.read"))
        console.log(`    ATTENZIONE in revisione: legge i messaggi e si collega a ${r.hosts.join(", ")}`);
    }
    failed ||= r.errors.length > 0;
  }
  return failed;
}

// ---- zip (senza compressione, date fisse: lo stesso sorgente dà lo stesso pacchetto) ----

const CRC_TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});

function crc32(buf) {
  let c = 0xffffffff;
  for (const b of buf) c = CRC_TABLE[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function zip(entries) {
  const DOS_DATE = (0 << 9) | (1 << 5) | 1; // 1 gennaio 1980
  const locals = [];
  const centrals = [];
  let offset = 0;
  for (const { name, data } of entries) {
    const nameBuf = Buffer.from(name, "utf8");
    const crc = crc32(data);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(10, 4); // versione necessaria
    local.writeUInt16LE(0x0800, 6); // nomi in UTF-8
    local.writeUInt16LE(0, 8); // nessuna compressione
    local.writeUInt16LE(0, 10);
    local.writeUInt16LE(DOS_DATE, 12);
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(data.length, 18);
    local.writeUInt32LE(data.length, 22);
    local.writeUInt16LE(nameBuf.length, 26);
    local.writeUInt16LE(0, 28);
    const central = Buffer.alloc(46);
    central.writeUInt32LE(0x02014b50, 0);
    central.writeUInt16LE(20, 4);
    central.writeUInt16LE(10, 6);
    central.writeUInt16LE(0x0800, 8);
    central.writeUInt16LE(0, 10);
    central.writeUInt16LE(0, 12);
    central.writeUInt16LE(DOS_DATE, 14);
    central.writeUInt32LE(crc, 16);
    central.writeUInt32LE(data.length, 20);
    central.writeUInt32LE(data.length, 24);
    central.writeUInt16LE(nameBuf.length, 28);
    central.writeUInt32LE(offset, 42);
    locals.push(local, nameBuf, data);
    centrals.push(central, nameBuf);
    offset += local.length + nameBuf.length + data.length;
  }
  const cd = Buffer.concat(centrals);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(entries.length, 8);
  end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(cd.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, cd, end]);
}

function pack(out, results) {
  mkdirSync(out, { recursive: true });
  const addons = [];
  for (const r of results) {
    const data = zip(r.files.map((f) => ({ name: f.rel, data: readFileSync(f.full) })));
    const file = `${r.manifest.id}-${r.manifest.version}.zip`;
    writeFileSync(join(out, file), data);
    addons.push({ ...r.manifest, file, size: data.length, sha256: createHash("sha256").update(data).digest("hex") });
    console.log(`  ${file}  ${data.length} byte`);
  }
  const catalog = { generated: new Date().toISOString(), addons };
  writeFileSync(join(out, "addons.json"), JSON.stringify(catalog, null, 2) + "\n");
  console.log(`  addons.json  ${addons.length} add-on`);
}

// ---- riga di comando ----

const [command, arg] = process.argv.slice(2);
try {
  if (command === "check") {
    const results = addonIds(arg).map(checkAddon);
    if (results.length === 0) console.log("Nessun add-on in addons/");
    process.exit(report(results) ? 1 : 0);
  } else if (command === "pack" && arg) {
    const results = addonIds().map(checkAddon);
    if (report(results)) {
      console.log("\nCi sono errori: nessun pacchetto scritto.");
      process.exit(1);
    }
    console.log(`\nPacchetti in ${arg}:`);
    pack(arg, results);
  } else {
    console.log("Uso: node scripts/addons.mjs check [id] | pack <cartella>");
    process.exit(2);
  }
} catch (e) {
  console.error(e.message);
  process.exit(1);
}

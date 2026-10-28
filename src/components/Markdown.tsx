import type { ReactNode } from "react";

/**
 * Il Markdown che Claude usa nelle risposte: paragrafi, titoli, elenchi,
 * tabelle, codice, grassetto e corsivo. Diventa elementi React, mai HTML:
 * il testo può citare parti di email scritte da chiunque.
 */
export function Markdown({ text }: { text: string }) {
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  const blocks: ReactNode[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (!line.trim()) {
      i++;
    } else if (line.startsWith("```")) {
      const code: string[] = [];
      for (i++; i < lines.length && !lines[i].startsWith("```"); i++) code.push(lines[i]);
      i++;
      blocks.push(<pre key={blocks.length}>{code.join("\n")}</pre>);
    } else if (/^#{1,6}\s/.test(line)) {
      blocks.push(
        <p key={blocks.length} className="md-title">
          {inline(line.replace(/^#+\s*/, ""))}
        </p>,
      );
      i++;
    } else if (LIST.test(line)) {
      const ordered = /^\s*\d+[.)]\s/.test(line);
      const items: string[] = [];
      for (; i < lines.length && LIST.test(lines[i]); i++) items.push(lines[i].replace(LIST, ""));
      const children = items.map((item, k) => <li key={k}>{inline(item)}</li>);
      blocks.push(ordered ? <ol key={blocks.length}>{children}</ol> : <ul key={blocks.length}>{children}</ul>);
    } else if (line.trimStart().startsWith("|")) {
      const rows: string[][] = [];
      for (; i < lines.length && lines[i].trimStart().startsWith("|"); i++) {
        const cells = lines[i].trim().replace(/^\||\|$/g, "").split("|").map((c) => c.trim());
        // La riga |---|---| separa l'intestazione.
        if (!cells.every((c) => /^:?-+:?$/.test(c))) rows.push(cells);
      }
      const [head, ...body] = rows;
      if (!head) continue;
      blocks.push(
        <div key={blocks.length} className="md-table">
          <table>
            <thead>
              <tr>{head.map((c, k) => <th key={k}>{inline(c)}</th>)}</tr>
            </thead>
            <tbody>
              {body.map((r, k) => (
                <tr key={k}>{r.map((c, j) => <td key={j}>{inline(c)}</td>)}</tr>
              ))}
            </tbody>
          </table>
        </div>,
      );
    } else {
      const para: string[] = [];
      for (; i < lines.length && lines[i].trim() && !SPECIAL.test(lines[i]); i++) para.push(lines[i]);
      blocks.push(
        <p key={blocks.length}>
          {para.map((l, k) => (
            <span key={k}>
              {k > 0 && <br />}
              {inline(l)}
            </span>
          ))}
        </p>,
      );
    }
  }
  return <div className="md">{blocks}</div>;
}

const LIST = /^\s*(?:[-*•]|\d+[.)])\s+/;
const SPECIAL = /^(?:```|#{1,6}\s|\s*(?:[-*•]|\d+[.)])\s+|\s*\|)/;
const INLINE = /(`[^`]+`|\*\*[^*]+\*\*|\[[^\]]+\]\([^)\s]+\)|\*[^*\s][^*]*\*)/g;

function inline(text: string): ReactNode[] {
  const out: ReactNode[] = [];
  let last = 0;
  for (const m of text.matchAll(INLINE)) {
    const token = m[0];
    const at = m.index ?? 0;
    if (at > last) out.push(text.slice(last, at));
    if (token.startsWith("`")) out.push(<code key={at}>{token.slice(1, -1)}</code>);
    else if (token.startsWith("**")) out.push(<strong key={at}>{token.slice(2, -2)}</strong>);
    // Dei link resta il testo: aprirli da qui non serve.
    else if (token.startsWith("[")) out.push(token.slice(1, token.indexOf("](")));
    else out.push(<em key={at}>{token.slice(1, -1)}</em>);
    last = at + token.length;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

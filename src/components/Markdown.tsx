import { ask } from "@tauri-apps/plugin-dialog";
import { Fragment, type ReactNode } from "react";
import { api } from "../lib/ipc";

/**
 * A small Markdown renderer for notes: headings, paragraphs, lists, quotes,
 * fenced code, inline code, bold, italics and links. It builds React elements
 * only (no HTML strings), so notes can't inject markup or scripts.
 */
export function Markdown({ text, codeActions }: { text: string; codeActions?: (code: string) => ReactNode }) {
  const blocks: ReactNode[] = [];
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  let i = 0;
  let key = 0;
  while (i < lines.length) {
    const line = lines[i];
    const fence = line.match(/^\s*(```|~~~)\s*([\w+-]*)\s*$/);
    if (fence) {
      const body: string[] = [];
      i++;
      while (i < lines.length && !lines[i].trim().startsWith(fence[1])) body.push(lines[i++]);
      i++;
      const code = body.join("\n");
      blocks.push(
        <div key={key++} className="group relative my-2">
          <pre className="overflow-x-auto rounded-md border border-border bg-surface-0 p-3 font-mono text-xs text-fg">{code}</pre>
          {codeActions && <div className="absolute right-1.5 top-1.5 flex gap-1 opacity-80 group-hover:opacity-100">{codeActions(code)}</div>}
        </div>,
      );
      continue;
    }
    const h = line.match(/^(#{1,4})\s+(.*)$/);
    if (h) {
      const size = ["text-lg", "text-base", "text-sm", "text-sm"][h[1].length - 1];
      blocks.push(
        <div key={key++} className={`${size} mb-1 mt-3 font-semibold first:mt-0`}>
          {inline(h[2])}
        </div>,
      );
      i++;
      continue;
    }
    if (/^\s*([-*+]|\d+[.)])\s+/.test(line)) {
      const ordered = /^\s*\d/.test(line);
      const items: string[] = [];
      while (i < lines.length && /^\s*([-*+]|\d+[.)])\s+/.test(lines[i])) items.push(lines[i++].replace(/^\s*([-*+]|\d+[.)])\s+/, ""));
      const List = ordered ? "ol" : "ul";
      blocks.push(
        <List key={key++} className={`my-1 space-y-0.5 pl-5 ${ordered ? "list-decimal" : "list-disc"}`}>
          {items.map((it, n) => (
            <li key={n}>{inline(it)}</li>
          ))}
        </List>,
      );
      continue;
    }
    if (/^\s*>/.test(line)) {
      const q: string[] = [];
      while (i < lines.length && /^\s*>/.test(lines[i])) q.push(lines[i++].replace(/^\s*>\s?/, ""));
      blocks.push(
        <blockquote key={key++} className="my-2 border-l-2 border-border pl-3 text-fg-muted">
          {inline(q.join(" "))}
        </blockquote>,
      );
      continue;
    }
    if (!line.trim()) {
      i++;
      continue;
    }
    const para: string[] = [];
    while (i < lines.length && lines[i].trim() && !/^(\s*(```|~~~)|#{1,4}\s|\s*([-*+]|\d+[.)])\s|\s*>)/.test(lines[i])) para.push(lines[i++]);
    blocks.push(
      <p key={key++} className="my-1.5">
        {inline(para.join(" "))}
      </p>,
    );
  }
  return <div className="text-sm leading-relaxed text-fg">{blocks}</div>;
}

async function openLink(url: string) {
  let u: URL;
  try {
    u = new URL(url);
  } catch {
    return;
  }
  if (u.protocol !== "http:" && u.protocol !== "https:") return;
  if (await ask(`Open this link in your browser?\n\n${u.href}`, { title: "Open link", okLabel: "Open" })) api.openUrl(u.href).catch(() => {});
}

/** Inline code, bold, italics and links. */
function inline(s: string): ReactNode {
  const out: ReactNode[] = [];
  const re = /(`[^`]+`)|(\*\*[^*]+\*\*)|(\*[^*]+\*|_[^_]+_)|(\[[^\]]+\]\([^)\s]+\))/g;
  let last = 0;
  let m: RegExpExecArray | null;
  let k = 0;
  while ((m = re.exec(s))) {
    if (m.index > last) out.push(<Fragment key={k++}>{s.slice(last, m.index)}</Fragment>);
    const t = m[0];
    if (m[1]) out.push(<code key={k++} className="rounded bg-surface-3 px-1 py-0.5 font-mono text-[0.85em]">{t.slice(1, -1)}</code>);
    else if (m[2]) out.push(<strong key={k++}>{t.slice(2, -2)}</strong>);
    else if (m[3]) out.push(<em key={k++}>{t.slice(1, -1)}</em>);
    else {
      const [, label, url] = t.match(/^\[([^\]]+)\]\(([^)\s]+)\)$/)!;
      out.push(
        <button key={k++} className="text-accent underline underline-offset-2 hover:text-accent-strong cursor-pointer" onClick={() => openLink(url)} title={url}>
          {label}
        </button>,
      );
    }
    last = m.index + t.length;
  }
  if (last < s.length) out.push(<Fragment key={k++}>{s.slice(last)}</Fragment>);
  return out;
}

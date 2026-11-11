import type { IDisposable, ILink, Terminal } from "@xterm/xterm";

/** Things in terminal output that can be clicked (M2.6). URLs are handled by the web-links addon. */
export type OutputLinkKind = "address" | "archive";

export interface OutputLink {
  kind: OutputLinkKind;
  text: string;
}

const URL_RE = /\bhttps?:\/\/[^\s"'<>]+/g;
const IPV4 = String.raw`(?:(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)\.){3}(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)`;
const PORT = String.raw`(?:6553[0-5]|655[0-2]\d|65[0-4]\d\d|6[0-4]\d{3}|[1-5]\d{4}|[1-9]\d{0,3})`;
const PATTERNS: { kind: OutputLinkKind; re: RegExp }[] = [
  // 10.0.0.5, 10.0.0.5:8080, localhost:3000, [::1]:5432
  { kind: "address", re: new RegExp(String.raw`(?<![\w.:/])(?:${IPV4}(?::${PORT})?|localhost:${PORT}|\[[0-9a-fA-F:]+\]:${PORT})(?![\w.:]*\w)`, "g") },
  // backup-2026.tar.gz, ./dist/app.zip
  { kind: "archive", re: /(?<![\w./~-])[\w./~+-]*[\w-]\.(?:tar\.gz|tar\.xz|tar\.bz2|tar\.zst|tgz|txz|tbz2|tar|zip|7z)(?=$|[\s'"),;:\]])/g },
];

/** Matches in one line of text, skipping anything inside a URL. Exported for tests. */
export function findOutputLinks(line: string): { kind: OutputLinkKind; text: string; start: number }[] {
  const urls = [...line.matchAll(URL_RE)].map((m) => [m.index!, m.index! + m[0].length]);
  const out: { kind: OutputLinkKind; text: string; start: number }[] = [];
  for (const { kind, re } of PATTERNS) {
    for (const m of line.matchAll(re)) {
      const s = m.index!;
      const e = s + m[0].length;
      if (urls.some(([a, b]) => s < b && e > a)) continue;
      out.push({ kind, text: m[0], start: s });
    }
  }
  return out;
}

/** The command that unpacks an archive (pasted, never run). */
export function extractCommand(name: string): string {
  const q = `'${name.replace(/'/g, `'\\''`)}'`;
  if (name.endsWith(".zip")) return `unzip ${q}`;
  if (name.endsWith(".7z")) return `7z x ${q}`;
  return `tar -xf ${q}`;
}

/** Whether an address means "this machine" (so it is the server's, in an SSH terminal). */
export function isLoopback(addr: string): boolean {
  return /^(localhost|127\.|0\.0\.0\.0|\[::1?\])/.test(addr);
}

/** Make addresses and archive names clickable; `onPick` shows the menu. */
export function registerOutputLinks(t: Terminal, onPick: (link: OutputLink, e: MouseEvent) => void): IDisposable {
  return t.registerLinkProvider({
    provideLinks(y, callback) {
      const line = t.buffer.active.getLine(y - 1);
      if (!line) return callback(undefined);
      const text = line.translateToString(true);
      const links: ILink[] = findOutputLinks(text).map((m) => ({
        range: { start: { x: m.start + 1, y }, end: { x: m.start + m.text.length, y } },
        text: m.text,
        decorations: { underline: true, pointerCursor: true },
        activate: (e) => onPick({ kind: m.kind, text: m.text }, e),
      }));
      callback(links.length ? links : undefined);
    },
  });
}

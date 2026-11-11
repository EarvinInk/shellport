import type { Host, PaneNode, PaneTarget, SavedCommand } from "./types";

export const uid = () => Math.random().toString(36).slice(2, 10);

export function leaf(id: string, target: PaneTarget): PaneNode {
  return { t: "pane", id, target };
}

export function allPanes(n: PaneNode): Extract<PaneNode, { t: "pane" }>[] {
  return n.t === "pane" ? [n] : [...allPanes(n.a), ...allPanes(n.b)];
}

/** Split `paneId`, putting a new pane with `target` to its right ("row") or below ("col"). */
export function splitPane(n: PaneNode, paneId: string, dir: "row" | "col", target: PaneTarget, newId = uid()): PaneNode {
  if (n.t === "pane") return n.id === paneId ? { t: "split", dir, ratio: 0.5, a: n, b: leaf(newId, target) } : n;
  return { ...n, a: splitPane(n.a, paneId, dir, target, newId), b: splitPane(n.b, paneId, dir, target, newId) };
}

/** Remove a pane; its sibling takes the space. Null when it was the last one. */
export function removePane(n: PaneNode, paneId: string): PaneNode | null {
  if (n.t === "pane") return n.id === paneId ? null : n;
  const a = removePane(n.a, paneId);
  const b = removePane(n.b, paneId);
  if (!a) return b;
  if (!b) return a;
  return { ...n, a, b };
}

/** Set the ratio of the split at `path` ("" = root, then "a"/"b" per level). */
export function setRatio(n: PaneNode, path: string, ratio: number): PaneNode {
  if (n.t === "pane") return n;
  if (!path) return { ...n, ratio };
  const [head, rest] = [path[0], path.slice(1)];
  return head === "a" ? { ...n, a: setRatio(n.a, rest, ratio) } : { ...n, b: setRatio(n.b, rest, ratio) };
}

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Pane rectangles and splitters, in percent of the tab area. */
export function layoutRects(n: PaneNode, r: Rect = { x: 0, y: 0, w: 100, h: 100 }, path = ""): { panes: { id: string; rect: Rect }[]; splits: { path: string; dir: "row" | "col"; rect: Rect; at: number }[] } {
  if (n.t === "pane") return { panes: [{ id: n.id, rect: r }], splits: [] };
  const ra = n.dir === "row" ? { ...r, w: r.w * n.ratio } : { ...r, h: r.h * n.ratio };
  const rb = n.dir === "row" ? { x: r.x + ra.w, y: r.y, w: r.w - ra.w, h: r.h } : { x: r.x, y: r.y + ra.h, w: r.w, h: r.h - ra.h };
  const A = layoutRects(n.a, ra, path + "a");
  const B = layoutRects(n.b, rb, path + "b");
  return { panes: [...A.panes, ...B.panes], splits: [{ path, dir: n.dir, rect: r, at: n.dir === "row" ? rb.x : rb.y }, ...A.splits, ...B.splits] };
}

/** Same layout with fresh pane ids (opening a workspace twice gives separate sessions). */
export function freshIds(n: PaneNode): PaneNode {
  return n.t === "pane" ? { ...n, id: uid() } : { ...n, a: freshIds(n.a), b: freshIds(n.b) };
}

/** Whether a saved command is offered on `host` (no scope = everywhere). */
export function commandAppliesTo(c: SavedCommand, host?: Host | null): boolean {
  const { hosts, groups } = c.scope ?? { hosts: [], groups: [] };
  if (!hosts.length && !groups.length) return true;
  if (!host) return false;
  return hosts.includes(host.id) || (!!host.group && groups.includes(host.group));
}

/** `{{name}}` / `{{name:default}}` placeholders in a command, in order, without duplicates. */
export function commandVariables(text: string): { name: string; def: string }[] {
  const seen = new Set<string>();
  const out: { name: string; def: string }[] = [];
  for (const m of text.matchAll(/\{\{\s*([A-Za-z_][\w-]*)\s*(?::([^}]*))?\}\}/g)) {
    if (!seen.has(m[1])) {
      seen.add(m[1]);
      out.push({ name: m[1], def: (m[2] ?? "").trim() });
    }
  }
  return out;
}

/** Fill placeholders; `{{host}}` and `{{user}}` come from the host. */
export function fillCommand(text: string, values: Record<string, string>, host?: Host | null): string {
  const builtin: Record<string, string> = host ? { host: host.hostname, user: host.username, name: host.name } : {};
  return text.replace(/\{\{\s*([A-Za-z_][\w-]*)\s*(?::([^}]*))?\}\}/g, (_, k: string, d?: string) => values[k] ?? builtin[k] ?? (d ?? "").trim());
}

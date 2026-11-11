import { ArrowDownUp, Columns2, Download, FolderOpen, History, LayoutGrid, Play, Radio, Rows2, ScrollText, SquareTerminal } from "lucide-react";
import { registerPaletteSource, type PaletteItem } from "../lib/palette";
import { commandAppliesTo } from "../lib/panes";
import { api } from "../lib/ipc";
import { terminalReader, useApp } from "../store";
import { activeTerminal, openWorkspace, pasteCommand } from "../views/CommandsView";
import { splitTab, toggleBroadcast } from "./PanesView";

const I = 14;

// M2 "work across servers": panes, saved commands, workspaces, runs, activity.
registerPaletteSource("work", () => {
  const s = useApp.getState();
  const items: PaletteItem[] = [];
  const active = s.tabs.find((t) => t.id === s.activeTab);
  const term = active && (active.kind === "local" || active.kind === "ssh" || active.kind === "panes") ? active : null;
  const group = "Terminals";
  if (term) {
    items.push(
      { id: "pane:split-right", title: "Split right", group, icon: <Columns2 size={I} />, keywords: "pane vertical", run: () => splitTab(term.id, "row") },
      { id: "pane:split-down", title: "Split down", group, icon: <Rows2 size={I} />, keywords: "pane horizontal", run: () => splitTab(term.id, "col") },
      {
        id: "pane:broadcast",
        title: s.broadcast[term.id] ? "Stop broadcasting" : "Broadcast typing to all panes",
        group,
        icon: <Radio size={I} />,
        keywords: "send input multiple synchronize",
        run: () => toggleBroadcast(term.id),
      },
    );
    for (const h of s.hosts) {
      items.push({
        id: `pane:host:${h.id}`,
        title: `Open ${h.name} in a split`,
        subtitle: `${h.username}@${h.hostname}`,
        group,
        production: h.production,
        icon: <Columns2 size={I} />,
        run: () => splitTab(term.id, "row", { kind: "ssh", hostId: h.id }),
      });
    }
  }
  const target = activeTerminal();
  for (const c of s.library.commands) {
    if (target && (!target.host || commandAppliesTo(c, target.host))) {
      items.push({ id: `cmd:paste:${c.id}`, title: `Paste: ${c.name}`, subtitle: c.command, group: "Commands", icon: <SquareTerminal size={I} />, run: () => pasteCommand(c) });
    }
    items.push({
      id: `cmd:run:${c.id}`,
      title: `Run on hosts: ${c.name}`,
      subtitle: c.command,
      group: "Commands",
      icon: <Play size={I} />,
      run: () => s.setRunDialog({ command: c.command, hostIds: c.scope.hosts }),
    });
  }
  for (const h of s.hosts) {
    for (const p of h.favoritePaths ?? []) {
      items.push({ id: `fav:${h.id}:${p}`, title: `${h.name}: ${p}`, group: "Favourite folders", production: h.production, icon: <FolderOpen size={I} />, keywords: "sftp files folder", run: () => s.gotoSftp(h.id, p) });
    }
  }
  for (const w of s.library.workspaces) {
    items.push({ id: `ws:${w.id}`, title: `Open workspace: ${w.name}`, group: "Workspaces", icon: <LayoutGrid size={I} />, run: () => openWorkspace(w.id) });
  }
  for (const h of s.hosts) {
    items.push({
      id: `activity:${h.id}`,
      title: `Activity: ${h.name}`,
      group: "Hosts",
      production: h.production,
      icon: <History size={I} />,
      keywords: "history log",
      run: () => s.openTab({ kind: "activity", title: `Activity: ${h.name}`, hostId: h.id }),
    });
  }
  const app = (id: string, title: string, icon: React.ReactNode, run: () => void, keywords = ""): PaletteItem => ({ id: `app:${id}`, title, icon, run, keywords, group: "Shellport" });
  items.push(
    app("run", "Run a command on several hosts…", <Play size={I} />, () => s.setRunDialog({ hostIds: [] }), "multi broadcast execute parallel"),
    app("commands", "Commands library", <ScrollText size={I} />, () => s.openTab({ kind: "commands", title: "Commands" }, { unique: true }), "snippets saved"),
    app("newcmd", "Save workspace (open terminals)…", <LayoutGrid size={I} />, () => s.openTab({ kind: "commands", title: "Commands" }, { unique: true }), "layout"),
    app("saveout", "Save terminal output…", <Download size={I} />, saveTerminalOutput, "scrollback export log file"),
    app("transfers", "Transfers", <ArrowDownUp size={I} />, () => s.setTransfersOpen(true), "uploads downloads copy queue progress"),
    app("activity", "Activity & history", <History size={I} />, () => s.openTab({ kind: "activity", title: "Activity" }, { unique: true }), "queries log runs transfers"),
  );
  return items;
});

/** Save the active terminal's scrollback and screen to a text file. */
async function saveTerminalOutput() {
  const st = useApp.getState();
  const target = activeTerminal();
  const read = target && terminalReader(target.paneId);
  if (!read) return st.toast("Open a terminal first, then save its output.", "error");
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, "0");
  const stamp = `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}_${p(d.getHours())}-${p(d.getMinutes())}`;
  const name = (target.host?.name ?? "local").replace(/[^\w.-]+/g, "_");
  try {
    const path = await api.textSave("Save terminal output", `${name}-${stamp}.txt`, read());
    if (path) st.toast(`Saved to ${path}`);
  } catch (e) {
    st.toast(`Couldn't save: ${e}`, "error");
  }
}

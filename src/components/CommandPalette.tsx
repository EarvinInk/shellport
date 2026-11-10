import clsx from "clsx";
import { Box, Database, FileDown, FileUp, FolderOpen, KeyRound, Lock, NotebookPen, PanelLeft, Pencil, Plus, Search, Server, Settings as SettingsIcon, ShieldCheck, SquareTerminal, Terminal, Unplug } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../lib/ipc";
import { paletteItems, recordUse, registerPaletteSource, searchPalette, type PaletteItem } from "../lib/palette";
import { useApp } from "../store";
import { ProdBadge } from "./ui";
import { openDb } from "./DatabasesSidebar";
import { openDocker, openSftp, openSsh } from "./HostsSidebar";

const I = 14;

// Core actions every installation has. Features add more with registerPaletteSource.
registerPaletteSource("core", () => {
  const s = useApp.getState();
  const items: PaletteItem[] = [];
  for (const t of s.tabs) {
    items.push({ id: `tab:${t.id}`, title: `Go to: ${t.title}`, group: "Open tabs", icon: <PanelLeft size={I} />, run: () => s.setActive(t.id) });
  }
  for (const h of s.hosts) {
    const sub = `${h.username}@${h.hostname}${h.port !== 22 ? `:${h.port}` : ""}`;
    const kw = [h.group, ...h.tags].join(" ");
    const base = { subtitle: sub, keywords: kw, group: "Hosts", production: h.production };
    items.push(
      { ...base, id: `ssh:${h.id}`, title: `Terminal: ${h.name}`, icon: <SquareTerminal size={I} />, run: () => openSsh(h) },
      { ...base, id: `sftp:${h.id}`, title: `Files: ${h.name}`, icon: <FolderOpen size={I} />, run: () => openSftp(h) },
      { ...base, id: `docker:${h.id}`, title: `Docker: ${h.name}`, icon: <Box size={I} />, run: () => openDocker(h.id, h.name) },
      { ...base, id: `notes:${h.id}`, title: `Notes: ${h.name}`, icon: <NotebookPen size={I} />, run: () => s.setNotesHost(h.id) },
      { ...base, id: `edit:${h.id}`, title: `Edit host: ${h.name}`, icon: <Pencil size={I} />, run: () => s.setHostEditor(h) },
      {
        ...base,
        id: `disc:${h.id}`,
        title: `Disconnect: ${h.name}`,
        icon: <Unplug size={I} />,
        run: () => api.hostDisconnect(h.id).then(() => s.toast(`Disconnected ${h.name}`)).catch(s.error),
      },
    );
  }
  for (const d of s.dbs) {
    const viaProd = !!s.hosts.find((h) => h.id === d.viaHostId)?.production;
    const base = { subtitle: `${d.username ? d.username + "@" : ""}${d.host}/${d.database}`, keywords: [d.group, ...d.tags].join(" "), group: "Databases", production: !!d.production || viaProd };
    items.push(
      { ...base, id: `db:${d.id}`, title: `Database: ${d.name}`, icon: <Database size={I} />, run: () => openDb(d) },
      { ...base, id: `dbedit:${d.id}`, title: `Edit database: ${d.name}`, icon: <Pencil size={I} />, run: () => s.setDbEditor(d) },
    );
  }
  const app = (id: string, title: string, icon: React.ReactNode, run: () => void, keywords = ""): PaletteItem => ({ id: `app:${id}`, title, icon, run, keywords, group: "Shellport" });
  items.push(
    app("local", "New local terminal", <Terminal size={I} />, () => s.openTab({ kind: "local", title: "Local" }), "shell"),
    app("newhost", "New host", <Plus size={I} />, () => s.setHostEditor("new"), "add server ssh"),
    app("newdb", "New database connection", <Plus size={I} />, () => s.setDbEditor("new"), "add postgres"),
    app("import", "Import hosts", <FileUp size={I} />, () => s.setImportOpen(true), "ssh config csv json"),
    app("exportjson", "Export hosts as JSON", <FileDown size={I} />, () => api.hostsExport("json").then((p) => p && s.toast(`Exported to ${p}`, "success")).catch(s.error)),
    app("exportcsv", "Export hosts as CSV", <FileDown size={I} />, () => api.hostsExport("csv").then((p) => p && s.toast(`Exported to ${p}`, "success")).catch(s.error)),
    app("vault", "Open vault", <ShieldCheck size={I} />, () => s.openTab({ kind: "vault", title: "Vault" }, { unique: true }), "secrets passwords keys"),
    app("lock", "Lock vault", <Lock size={I} />, () => api.vaultLock().then(() => s.refreshVault()).then(() => s.toast("Vault locked"))),
    app("settings", "Settings", <SettingsIcon size={I} />, () => s.openTab({ kind: "settings", title: "Settings" }, { unique: true }), "preferences backup"),
    app("sidebar-hosts", "Show hosts", <Server size={I} />, () => s.setSidebar("hosts")),
    app("sidebar-dbs", "Show databases", <Database size={I} />, () => s.setSidebar("databases")),
    app("sidebar-docker", "Show Docker", <Box size={I} />, () => s.setSidebar("docker")),
    app("keys", "Generate an SSH key", <KeyRound size={I} />, () => s.openTab({ kind: "vault", title: "Vault" }, { unique: true }), "ed25519"),
  );
  return items;
});

/** Ctrl+Shift+P: one place to reach every host, tab and action. */
export function CommandPalette() {
  const open = useApp((s) => s.paletteOpen);
  const setOpen = useApp((s) => s.setPaletteOpen);
  const [q, setQ] = useState("");
  const [sel, setSel] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);
  const all = useMemo(() => (open ? paletteItems() : []), [open]);
  const results = useMemo(() => searchPalette(all, q), [all, q]);

  useEffect(() => {
    if (open) {
      setQ("");
      setSel(0);
    }
  }, [open]);
  useEffect(() => setSel(0), [q]);
  useEffect(() => {
    listRef.current?.querySelector(`[data-i="${sel}"]`)?.scrollIntoView({ block: "nearest" });
  }, [sel]);

  if (!open) return null;
  const choose = (it: PaletteItem | undefined) => {
    if (!it) return;
    recordUse(it.id);
    setOpen(false);
    it.run();
  };

  let lastGroup = "";
  return (
    <div className="fixed inset-0 z-50 flex justify-center bg-black/40 pt-[12vh]" onMouseDown={() => setOpen(false)}>
      <div
        className="flex h-fit max-h-[70vh] w-full max-w-xl flex-col overflow-hidden rounded-xl border border-border bg-surface-2 shadow-2xl"
        onMouseDown={(e) => e.stopPropagation()}
        role="dialog"
        aria-label="Command palette"
      >
        <div className="flex items-center gap-2 border-b border-border px-3">
          <Search size={15} className="text-fg-subtle" />
          <input
            autoFocus
            value={q}
            onChange={(e) => setQ(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") (e.preventDefault(), setSel((i) => Math.min(i + 1, results.length - 1)));
              else if (e.key === "ArrowUp") (e.preventDefault(), setSel((i) => Math.max(i - 1, 0)));
              else if (e.key === "Enter") (e.preventDefault(), choose(results[sel]));
              else if (e.key === "Escape") (e.preventDefault(), setOpen(false));
            }}
            placeholder="Search hosts, databases, tabs and actions…"
            className="h-12 flex-1 bg-transparent text-sm text-fg outline-none placeholder:text-fg-subtle"
            aria-controls="palette-list"
          />
          <kbd className="rounded border border-border px-1.5 py-0.5 font-mono text-[10px] text-fg-subtle">Esc</kbd>
        </div>
        <div ref={listRef} id="palette-list" role="listbox" className="min-h-0 overflow-y-auto p-1.5">
          {results.length === 0 && <div className="px-3 py-6 text-center text-sm text-fg-subtle">No matches</div>}
          {results.map((it, i) => {
            const header = !q && it.group !== lastGroup ? it.group : null;
            lastGroup = it.group;
            return (
              <div key={it.id + i}>
                {header && <div className="px-2.5 pb-1 pt-2 text-[11px] font-semibold uppercase tracking-wider text-fg-subtle">{header}</div>}
                <button
                  data-i={i}
                  role="option"
                  aria-selected={i === sel}
                  onMouseMove={() => setSel(i)}
                  onClick={() => choose(it)}
                  className={clsx("flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left cursor-pointer", i === sel ? "bg-accent/20 text-fg" : "text-fg-muted")}
                >
                  <span className="shrink-0 text-fg-subtle">{it.icon}</span>
                  <span className="min-w-0 flex-1 truncate text-sm text-fg">{it.title}</span>
                  {it.production && <ProdBadge />}
                  {it.subtitle && <span className="max-w-[45%] shrink truncate text-xs text-fg-subtle">{it.subtitle}</span>}
                </button>
              </div>
            );
          })}
        </div>
        <div className="flex gap-3 border-t border-border px-3 py-1.5 text-[11px] text-fg-subtle">
          <span>↑↓ choose</span>
          <span>Enter run</span>
          <span className="ml-auto">Most used first</span>
        </div>
      </div>
    </div>
  );
}

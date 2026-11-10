import { Database, Network, Pencil, Play, Plus, Search } from "lucide-react";
import { useMemo, useState } from "react";
import { DB_ENGINES } from "../lib/dbEngines";
import type { DbProfile } from "../lib/types";
import { useApp } from "../store";
import { IconButton, Input, ProdBadge } from "./ui";

/** Open a new tab for the profile; extra tabs on the same database are numbered. */
export function openDb(d: DbProfile) {
  const { tabs, openTab } = useApp.getState();
  const n = tabs.filter((t) => t.kind === "db" && t.dbId === d.id).length;
  openTab({ kind: "db", title: n ? `${d.name} · ${n + 1}` : d.name, dbId: d.id });
}

export function DatabasesSidebar() {
  const { dbs, setDbEditor } = useApp();
  const [q, setQ] = useState("");
  const groups = useMemo(() => {
    const ql = q.toLowerCase();
    const m = new Map<string, DbProfile[]>();
    for (const d of dbs) {
      if (ql && ![d.name, d.host, d.database, d.username, d.group, ...d.tags].some((s) => s?.toLowerCase().includes(ql))) continue;
      const g = d.group || "Databases";
      m.set(g, [...(m.get(g) ?? []), d]);
    }
    return [...m.entries()];
  }, [dbs, q]);

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between px-3 pb-2 pt-3">
        <span className="text-xs font-semibold uppercase tracking-wider text-fg-subtle">Databases</span>
        <IconButton title="New database connection" onClick={() => setDbEditor("new")}>
          <Plus size={16} />
        </IconButton>
      </div>
      <div className="px-3 pb-2">
        <div className="relative">
          <Search size={14} className="absolute left-2 top-2 text-fg-subtle" />
          <Input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Filter databases" className="pl-7" />
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-3">
        {groups.length === 0 && (
          <div className="px-2 py-6 text-center text-sm text-fg-subtle">
            {dbs.length === 0 ? "No databases yet. Add a connection, directly or through one of your SSH hosts." : "No matches"}
          </div>
        )}
        {groups.map(([g, list]) => (
          <div key={g} className="mb-2">
            <div className="px-2 py-1 text-[11px] font-semibold uppercase tracking-wider text-fg-subtle">{g}</div>
            {list.map((d) => (
              <DbRow key={d.id} d={d} />
            ))}
          </div>
        ))}
      </div>
    </div>
  );
}

function DbRow({ d }: { d: DbProfile }) {
  const { setDbEditor } = useApp();
  const viaHost = useApp((s) => s.hosts.find((h) => h.id === d.viaHostId));
  const via = viaHost?.name;
  const production = !!d.production || !!viaHost?.production;
  const live = useApp((s) => s.dbLive[d.id] ?? 0);
  const txOpen = useApp((s) => s.dbTx[d.id] ?? 0);
  const engine = DB_ENGINES[d.engine];
  return (
    <div
      className="group relative flex select-none items-center gap-2 rounded-md px-2 py-1.5 hover:bg-surface-3 cursor-pointer"
      onDoubleClick={() => openDb(d)}
      onKeyDown={(e) => e.key === "Enter" && openDb(d)}
      tabIndex={0}
      title="Double-click or press Enter to open"
    >
      <div
        className="relative flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-surface-3 text-fg-muted"
        style={d.color ? { background: d.color + "33", color: d.color } : undefined}
      >
        <Database size={14} />
        {live > 0 && (
          <span
            className={"absolute -right-0.5 -top-0.5 h-2.5 w-2.5 rounded-full border-2 border-surface-1 " + (txOpen ? "bg-warn" : "bg-ok")}
            title={txOpen ? "Open transaction: commit or roll back in its tab" : `${live} open tab${live === 1 ? "" : "s"}`}
          />
        )}
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5 text-sm">
          <span className="truncate">{d.name}</span>
          {production && <ProdBadge />}
          {via && (
            <span className="inline-flex shrink-0 items-center gap-0.5 text-[11px] text-accent" title={`Through SSH host ${via}`}>
              <Network size={10} /> {via}
            </span>
          )}
        </div>
        <div className="truncate text-[11px] text-fg-subtle">
          {engine?.label ?? d.engine} · {d.username ? `${d.username}@` : ""}
          {d.host}
          {d.port !== engine?.defaultPort ? `:${d.port}` : ""}
          {d.database ? `/${d.database}` : ""}
        </div>
      </div>
      {/* Overlay the actions so they never squeeze the name. */}
      <div className="absolute right-1 top-1/2 hidden -translate-y-1/2 rounded-md bg-surface-3 pl-1 shadow-[-8px_0_8px_var(--color-surface-3)] group-hover:flex group-focus-within:flex">
        <IconButton title="Open" onClick={() => openDb(d)}>
          <Play size={14} />
        </IconButton>
        <IconButton title="Edit" onClick={() => setDbEditor(d)}>
          <Pencil size={14} />
        </IconButton>
      </div>
    </div>
  );
}

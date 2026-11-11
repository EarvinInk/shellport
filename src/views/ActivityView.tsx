import { ask } from "@tauri-apps/plugin-dialog";
import clsx from "clsx";
import { ArrowLeftRight, CheckCircle2, ChevronDown, ChevronRight, Database, History, Play, Search, Trash2, XCircle } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Button, Empty, IconButton, Input, Select } from "../components/ui";
import { api } from "../lib/ipc";
import type { ActivityEntry, ActivityKind } from "../lib/types";
import { useApp } from "../store";

const KIND_LABEL: Record<ActivityKind, string> = { query: "Query", multiRun: "Run on hosts", transfer: "Transfer", fileAction: "File action" };

export function when(ms: number): string {
  const d = new Date(ms);
  const today = new Date();
  const time = d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  return d.toDateString() === today.toDateString() ? time : `${d.toLocaleDateString(undefined, { month: "short", day: "numeric" })} ${time}`;
}

function dur(ms?: number | null) {
  if (ms == null) return "";
  return ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(1)} s`;
}

/** Open SQL in a new editor tab on that database. */
export function openQueryInEditor(dbId: string, sql: string) {
  const { dbs, openTab, toast } = useApp.getState();
  const db = dbs.find((d) => d.id === dbId);
  if (!db) return toast("That database connection no longer exists.", "error");
  const id = openTab({ kind: "db", title: db.name, dbId });
  try {
    localStorage.setItem(`shellport.dbsql.tab.${id}`, sql);
  } catch {
    /* storage unavailable: the tab opens with its usual text */
  }
}

export function ActivityView({ hostId: initialHost, dbId: initialDb }: { hostId?: string; dbId?: string }) {
  const { hosts, dbs, error, setRunDialog } = useApp();
  const [kind, setKind] = useState<ActivityKind | "">("");
  const [scope, setScope] = useState(initialHost ? `h:${initialHost}` : initialDb ? `d:${initialDb}` : "");
  const [q, setQ] = useState("");
  const [entries, setEntries] = useState<ActivityEntry[]>([]);
  const [open, setOpen] = useState<Record<string, boolean>>({});

  const load = useCallback(() => {
    api
      .activityList({
        kind: kind || undefined,
        hostId: scope.startsWith("h:") ? scope.slice(2) : undefined,
        dbId: scope.startsWith("d:") ? scope.slice(2) : undefined,
        search: q || undefined,
        limit: 500,
      })
      .then(setEntries)
      .catch(error);
  }, [kind, scope, q, error]);
  useEffect(() => {
    const t = setTimeout(load, 150);
    return () => clearTimeout(t);
  }, [load]);
  // New entries appear when the tab is visited again or Refresh is pressed;
  // also refresh every few seconds while shown, cheaply.
  useEffect(() => {
    const t = setInterval(load, 5000);
    return () => clearInterval(t);
  }, [load]);

  const clear = async () => {
    const h = scope.startsWith("h:") ? scope.slice(2) : undefined;
    const d = scope.startsWith("d:") ? scope.slice(2) : undefined;
    const what = h ? `for ${hosts.find((x) => x.id === h)?.name ?? "this host"}` : d ? `for ${dbs.find((x) => x.id === d)?.name ?? "this database"}` : "everywhere";
    if (!(await ask(`Clear the history ${what}? This can't be undone.`, { title: "Clear history", kind: "warning", okLabel: "Clear" }))) return;
    await api.activityClear(h, d).catch(error);
    load();
  };

  const icon = (e: ActivityEntry) => (e.kind === "query" ? <Database size={14} /> : e.kind === "multiRun" ? <Play size={14} /> : <ArrowLeftRight size={14} />);

  return (
    <div className="flex h-full flex-col">
      <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-border px-4 py-2.5">
        <Select value={kind} onChange={(e) => setKind(e.target.value as ActivityKind | "")} className="h-8 w-40">
          <option value="">Everything</option>
          <option value="query">Queries</option>
          <option value="multiRun">Runs on hosts</option>
          <option value="transfer">Transfers</option>
          <option value="fileAction">Compress and extract</option>
        </Select>
        <Select value={scope} onChange={(e) => setScope(e.target.value)} className="h-8 w-52">
          <option value="">All hosts and databases</option>
          {hosts.length > 0 && (
            <optgroup label="Hosts">
              {hosts.map((h) => (
                <option key={h.id} value={`h:${h.id}`}>
                  {h.name}
                </option>
              ))}
            </optgroup>
          )}
          {dbs.length > 0 && (
            <optgroup label="Databases">
              {dbs.map((d) => (
                <option key={d.id} value={`d:${d.id}`}>
                  {d.name}
                </option>
              ))}
            </optgroup>
          )}
        </Select>
        <div className="relative w-64">
          <Search size={13} className="absolute left-2 top-2.5 text-fg-subtle" />
          <Input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Search SQL, commands, files" className="h-8 pl-7" />
        </div>
        <div className="flex-1" />
        <Button size="sm" variant="ghost" onClick={clear} disabled={!entries.length}>
          <Trash2 size={13} /> Clear…
        </Button>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {entries.length === 0 ? (
          <Empty icon={<History size={28} />} title="Nothing here yet">
            Queries you run, commands run on several hosts and file transfers are listed here. History stays on this machine.
          </Empty>
        ) : (
          <div className="divide-y divide-border">
            {entries.map((e) => {
              const expanded = !!open[e.id];
              const okCount = e.results.filter((r) => r.ok).length;
              return (
                <div key={e.id} className="group px-4 py-2">
                  <div className="flex items-start gap-3">
                    <span className={clsx("mt-0.5", e.ok ? "text-fg-subtle" : "text-danger")}>{icon(e)}</span>
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2 text-xs text-fg-subtle">
                        <span className="tabular-nums">{when(e.at)}</span>
                        <span>{KIND_LABEL[e.kind]}</span>
                        <span className="font-medium text-fg-muted">{e.title}</span>
                        {e.kind === "multiRun" ? (
                          <span className={okCount === e.results.length ? "text-ok" : "text-danger"}>
                            {okCount} of {e.results.length} OK
                          </span>
                        ) : (
                          !e.ok && <span className="text-danger">failed</span>
                        )}
                        {e.count != null && e.kind === "query" && <span>{e.count} rows</span>}
                        {e.durationMs != null && <span>{dur(e.durationMs)}</span>}
                      </div>
                      <button
                        className="mt-0.5 flex w-full min-w-0 items-start gap-1 text-left"
                        onClick={() => setOpen({ ...open, [e.id]: !expanded })}
                        title={expanded ? "Show less" : "Show more"}
                      >
                        {expanded ? <ChevronDown size={13} className="mt-0.5 shrink-0" /> : <ChevronRight size={13} className="mt-0.5 shrink-0" />}
                        <pre className={clsx("min-w-0 font-mono text-xs", expanded ? "whitespace-pre-wrap break-all" : "truncate")}>{e.detail}</pre>
                      </button>
                      {e.message && <div className={clsx("mt-1 text-xs", e.ok ? "text-fg-subtle" : "text-danger")}>{e.message}</div>}
                      {expanded && e.results.length > 0 && (
                        <div className="mt-2 space-y-2">
                          {e.results.map((r) => (
                            <div key={r.hostId} className="rounded-md border border-border">
                              <div className="flex items-center gap-2 border-b border-border px-2.5 py-1 text-xs">
                                {r.ok ? <CheckCircle2 size={13} className="text-ok" /> : <XCircle size={13} className="text-danger" />}
                                <span className="font-medium">{r.hostName}</span>
                                {r.exitCode != null && <span className="text-fg-subtle">exit {r.exitCode}</span>}
                                <span className="text-fg-subtle">{dur(r.durationMs)}</span>
                              </div>
                              <pre className="max-h-60 overflow-auto whitespace-pre-wrap break-all p-2.5 font-mono text-xs text-fg-muted">{r.output || "(no output)"}</pre>
                            </div>
                          ))}
                        </div>
                      )}
                    </div>
                    <div className="flex shrink-0 gap-0.5 opacity-0 group-hover:opacity-100">
                      {e.kind === "query" && e.dbId && (
                        <Button size="sm" variant="ghost" className="h-7" onClick={() => openQueryInEditor(e.dbId!, e.detail)}>
                          Open in editor
                        </Button>
                      )}
                      {e.kind === "multiRun" && (
                        <Button size="sm" variant="ghost" className="h-7" onClick={() => setRunDialog({ command: e.detail, hostIds: e.results.map((r) => r.hostId) })}>
                          Run again…
                        </Button>
                      )}
                      <IconButton
                        title="Remove from history"
                        onClick={async () => {
                          await api.activityDelete(e.id).catch(error);
                          load();
                        }}
                      >
                        <Trash2 size={13} />
                      </IconButton>
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}

import { Channel } from "@tauri-apps/api/core";
import clsx from "clsx";
import { CheckCircle2, ChevronDown, ChevronRight, Loader2, Search, XCircle } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../lib/ipc";
import { commandVariables, fillCommand } from "../lib/panes";
import type { HostResult, RunEvent } from "../lib/types";
import { useApp } from "../store";
import { Button, Checkbox, Field, Input, Modal, ProdBadge, Select, TextArea } from "./ui";

const BUILTINS = ["host", "user", "name"];
const KEEP = Object.fromEntries(BUILTINS.map((b) => [b, `{{${b}}}`]));

type Phase = { kind: "edit" } | { kind: "running"; runId: string } | { kind: "done"; ok: number; failed: number };

/**
 * Run one command on several hosts and collect each host's output (M2.3).
 * Nothing starts until Run is pressed; production hosts need a tick first.
 */
export function RunDialog() {
  const req = useApp((s) => s.runDialog);
  if (!req) return null;
  return <RunDialogInner initial={req} />;
}

function RunDialogInner({ initial }: { initial: { hostIds?: string[]; command?: string } }) {
  const { hosts, library, setRunDialog, openTab, error } = useApp();
  const [picked, setPicked] = useState<Set<string>>(new Set(initial.hostIds ?? []));
  const [command, setCommand] = useState(initial.command ?? "");
  const [vals, setVals] = useState<Record<string, string>>({});
  const [timeout, setTimeoutSecs] = useState(60);
  const [q, setQ] = useState("");
  const [prodOk, setProdOk] = useState(false);
  const [phase, setPhase] = useState<Phase>({ kind: "edit" });
  const [running, setRunning] = useState<Set<string>>(new Set());
  const [results, setResults] = useState<HostResult[]>([]);
  const [open, setOpen] = useState<Record<string, boolean>>({});
  const activityId = useRef<string | null>(null);

  const vars = useMemo(() => commandVariables(command).filter((v) => !BUILTINS.includes(v.name)), [command]);
  useEffect(() => setVals((old) => Object.fromEntries(vars.map((v) => [v.name, old[v.name] ?? v.def]))), [vars]);

  const groups = useMemo(() => {
    const m = new Map<string, typeof hosts>();
    for (const h of hosts) {
      const g = h.group || "";
      if (!m.has(g)) m.set(g, []);
      m.get(g)!.push(h);
    }
    return [...m.entries()].sort(([a], [b]) => (a === "" ? 1 : b === "" ? -1 : a.localeCompare(b)));
  }, [hosts]);
  const chosen = hosts.filter((h) => picked.has(h.id));
  const prod = chosen.filter((h) => h.production);
  const finalCmd = fillCommand(command, { ...vals, ...KEEP });
  const canRun = chosen.length > 0 && command.trim() && (!prod.length || prodOk) && phase.kind !== "running";

  const toggle = (ids: string[], on: boolean) => {
    const n = new Set(picked);
    ids.forEach((id) => (on ? n.add(id) : n.delete(id)));
    setPicked(n);
    setProdOk(false);
  };

  const run = async () => {
    setResults([]);
    setRunning(new Set());
    setOpen({});
    const ch = new Channel<RunEvent>();
    ch.onmessage = (ev) => {
      if (ev.type === "started") setRunning((s) => new Set(s).add(ev.hostId));
      else if (ev.type === "done") {
        setRunning((s) => {
          const n = new Set(s);
          n.delete(ev.result.hostId);
          return n;
        });
        setResults((r) => [...r, ev.result]);
      } else {
        activityId.current = ev.activityId;
        setPhase({ kind: "done", ok: ev.ok, failed: ev.failed });
      }
    };
    try {
      const runId = await api.hostsRun(chosen.map((h) => h.id), finalCmd, timeout, prod.length > 0 && prodOk, ch);
      setPhase((p) => (p.kind === "done" ? p : { kind: "running", runId }));
    } catch (e) {
      error(e);
      setPhase({ kind: "edit" });
    }
  };

  const cancel = async () => {
    if (phase.kind !== "running") return;
    await api.taskCancel(`run:${phase.runId}`).catch(() => {});
    setRunning(new Set());
    setPhase({ kind: "done", ok: results.filter((r) => r.ok).length, failed: results.filter((r) => !r.ok).length });
    useApp.getState().toast("Run cancelled; hosts already started may finish their command.", "info");
  };

  const close = () => {
    if (phase.kind === "running") return;
    setRunDialog(null);
  };

  const shown = q ? groups.map(([g, hs]) => [g, hs.filter((h) => (h.name + " " + h.hostname).toLowerCase().includes(q.toLowerCase()))] as const).filter(([, hs]) => hs.length) : groups;
  const pending = chosen.filter((h) => !results.some((r) => r.hostId === h.id));

  return (
    <Modal
      title="Run on hosts"
      onClose={close}
      width="max-w-3xl"
      footer={
        <>
          {phase.kind === "done" && (
            <span className={clsx("mr-auto text-sm", phase.failed ? "text-danger" : "text-ok")}>
              {phase.ok} of {phase.ok + phase.failed} OK
            </span>
          )}
          {phase.kind === "done" && (
            <Button
              variant="ghost"
              onClick={() => {
                openTab({ kind: "activity", title: "Activity" }, { unique: true });
                setRunDialog(null);
              }}
            >
              Open Activity
            </Button>
          )}
          {phase.kind === "running" ? (
            <Button variant="danger" onClick={cancel}>
              Cancel run
            </Button>
          ) : (
            <Button variant="ghost" onClick={close}>
              Close
            </Button>
          )}
          <Button variant="primary" onClick={run} disabled={!canRun}>
            {phase.kind === "done" ? "Run again" : `Run on ${chosen.length} host${chosen.length === 1 ? "" : "s"}`}
          </Button>
        </>
      }
    >
      <div className="space-y-4">
        <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1.3fr)] gap-4">
          <div className="flex min-h-0 flex-col gap-2">
            <div className="flex items-center justify-between text-xs font-medium text-fg-muted">
              <span>Hosts ({chosen.length} chosen)</span>
              {picked.size > 0 && (
                <button className="text-accent hover:underline" onClick={() => toggle([...picked], false)}>
                  Clear
                </button>
              )}
            </div>
            <div className="relative">
              <Search size={13} className="absolute left-2 top-2.5 text-fg-subtle" />
              <Input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Filter hosts" className="h-8 pl-7" />
            </div>
            <div className="max-h-64 space-y-2 overflow-y-auto rounded-md border border-border p-2">
              {hosts.length === 0 && <div className="p-2 text-xs text-fg-subtle">Add hosts first.</div>}
              {shown.map(([g, hs]) => {
                const all = hs.every((h) => picked.has(h.id));
                return (
                  <div key={g || "-"}>
                    <div className="mb-1 flex items-center gap-2">
                      <Checkbox label={g || "No group"} checked={all} onChange={(v) => toggle(hs.map((h) => h.id), v)} />
                    </div>
                    <div className="space-y-1 pl-5">
                      {hs.map((h) => (
                        <div key={h.id} className="flex items-center gap-2">
                          <Checkbox label={h.name} checked={picked.has(h.id)} onChange={(v) => toggle([h.id], v)} />
                          {h.production && <ProdBadge />}
                        </div>
                      ))}
                    </div>
                  </div>
                );
              })}
            </div>
          </div>
          <div className="space-y-3">
            {library.commands.length > 0 && (
              <Field label="Saved command">
                <Select
                  value=""
                  onChange={(e) => {
                    const c = library.commands.find((x) => x.id === e.target.value);
                    if (c) setCommand(c.command);
                  }}
                >
                  <option value="">Choose one to fill in the command…</option>
                  {library.commands.map((c) => (
                    <option key={c.id} value={c.id}>
                      {c.name}
                    </option>
                  ))}
                </Select>
              </Field>
            )}
            <Field label="Command" hint="Runs in each host's non-interactive shell; {{host}}, {{user}} and {{name}} are filled per host.">
              <TextArea rows={3} value={command} onChange={(e) => setCommand(e.target.value)} spellCheck={false} placeholder="uptime" className="font-mono" />
            </Field>
            {vars.map((v) => (
              <Field key={v.name} label={v.name}>
                <Input value={vals[v.name] ?? ""} onChange={(e) => setVals({ ...vals, [v.name]: e.target.value })} />
              </Field>
            ))}
            <Field label="Time limit per host">
              <Select value={String(timeout)} onChange={(e) => setTimeoutSecs(Number(e.target.value))} className="w-40">
                {[10, 30, 60, 300, 900, 3600].map((s) => (
                  <option key={s} value={s}>
                    {s < 60 ? `${s} seconds` : `${s / 60} minute${s === 60 ? "" : "s"}`}
                  </option>
                ))}
              </Select>
            </Field>
          </div>
        </div>

        {prod.length > 0 && phase.kind !== "running" && (
          <div className="rounded-md border border-danger/50 bg-danger/10 p-3 text-sm">
            <div className="mb-2">
              This will run on <b>{prod.length}</b> production host{prod.length === 1 ? "" : "s"}: {prod.map((h) => h.name).join(", ")}.
            </div>
            <Checkbox label="I understand; run it there too" checked={prodOk} onChange={setProdOk} />
          </div>
        )}

        {phase.kind !== "edit" && (
          <div className="space-y-1.5">
            {[...results].sort((a, b) => Number(a.ok) - Number(b.ok)).map((r) => (
              <div key={r.hostId} className="rounded-md border border-border">
                <button className="flex w-full items-center gap-2 px-2.5 py-1.5 text-left text-xs" onClick={() => setOpen({ ...open, [r.hostId]: !open[r.hostId] })}>
                  {open[r.hostId] ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
                  {r.ok ? <CheckCircle2 size={13} className="text-ok" /> : <XCircle size={13} className="text-danger" />}
                  <span className="font-medium">{r.hostName}</span>
                  {r.exitCode != null && <span className="text-fg-subtle">exit {r.exitCode}</span>}
                  <span className="min-w-0 flex-1 truncate font-mono text-fg-subtle">{r.output.trim().split("\n").pop()}</span>
                </button>
                {open[r.hostId] && <pre className="max-h-60 overflow-auto whitespace-pre-wrap break-all border-t border-border p-2.5 font-mono text-xs text-fg-muted">{r.output || "(no output)"}</pre>}
              </div>
            ))}
            {phase.kind === "running" &&
              pending.map((h) => (
                <div key={h.id} className="flex items-center gap-2 rounded-md border border-border px-2.5 py-1.5 text-xs text-fg-subtle">
                  {running.has(h.id) ? <Loader2 size={13} className="animate-spin" /> : <span className="w-[13px]" />}
                  {h.name} {running.has(h.id) ? "running…" : "waiting"}
                </div>
              ))}
          </div>
        )}
      </div>
    </Modal>
  );
}

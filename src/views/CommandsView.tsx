import { ask } from "@tauri-apps/plugin-dialog";
import { LayoutGrid, Pencil, Play, Plus, ScrollText, Search, SquareTerminal, Trash2 } from "lucide-react";
import { useMemo, useState } from "react";
import { focusedPane } from "../components/PanesView";
import { Button, Checkbox, Empty, Field, IconButton, Input, Modal, Select, TextArea } from "../components/ui";
import { api } from "../lib/ipc";
import { allPanes, commandAppliesTo, commandVariables, fillCommand, freshIds } from "../lib/panes";
import type { Host, SavedCommand } from "../lib/types";
import { terminalPaster, useApp, type Tab } from "../store";

const blank = (): SavedCommand => ({ id: "", name: "", command: "", description: "", scope: { hosts: [], groups: [] }, runOnConnect: false });

/** The terminal pane a command would go to: the focused pane of the active terminal tab. */
export function activeTerminal(): { paneId: string; host: Host | null } | null {
  const { tabs, activeTab, hosts } = useApp.getState();
  const t = tabs.find((x) => x.id === activeTab);
  if (!t) return null;
  if (t.kind === "local") return { paneId: t.id, host: null };
  if (t.kind === "ssh") return { paneId: t.id, host: hosts.find((h) => h.id === t.hostId) ?? null };
  if (t.kind !== "panes") return null;
  const panes = allPanes(t.layout);
  const p = panes.find((x) => x.id === focusedPane(t.id)) ?? panes[0];
  return { paneId: p.id, host: p.target.kind === "ssh" ? (hosts.find((h) => h.id === (p.target as { hostId: string }).hostId) ?? null) : null };
}

/** Ask for a command's `{{variables}}` (resolves null if cancelled). */
let askVars: ((cmd: SavedCommand, host: Host | null) => Promise<string | null>) | null = null;
export function fillVariables(cmd: SavedCommand, host: Host | null): Promise<string | null> {
  const vars = commandVariables(cmd.command).filter((v) => !(host && ["host", "user", "name"].includes(v.name)));
  if (!vars.length || !askVars) return Promise.resolve(fillCommand(cmd.command, {}, host));
  return askVars(cmd, host);
}

/** Paste a saved command into the active terminal (never presses Enter). */
export async function pasteCommand(cmd: SavedCommand) {
  const target = activeTerminal();
  if (!target) return useApp.getState().toast("Open a terminal first, then paste the command into it.", "error");
  const text = await fillVariables(cmd, target.host);
  if (text == null) return;
  const paste = terminalPaster(target.paneId);
  if (!paste) return useApp.getState().toast("That terminal isn't connected yet.", "error");
  await paste(text);
}

/** Open a saved workspace: one new tab per saved tab, with fresh sessions. */
export function openWorkspace(id: string) {
  const { library, hosts, toast } = useApp.getState();
  const w = library.workspaces.find((x) => x.id === id);
  if (!w) return;
  const known = new Set(hosts.map((h) => h.id));
  let skipped = 0;
  for (const t of w.layout.tabs) {
    const layout = freshIds(t.layout);
    if (allPanes(layout).some((p) => p.target.kind === "ssh" && !known.has(p.target.hostId))) {
      skipped++;
      continue;
    }
    useApp.getState().openTab({ kind: "panes", title: t.title, layout } as Omit<Extract<Tab, { kind: "panes" }>, "id">);
  }
  if (skipped) toast(`${skipped} tab${skipped === 1 ? "" : "s"} skipped: their hosts no longer exist`, "info");
}

/** Save the open terminal tabs (with their panes) as a workspace. */
export async function saveWorkspace(name: string) {
  const { tabs, refreshLibrary, toast, error } = useApp.getState();
  const terms = tabs.flatMap((t) =>
    t.kind === "panes"
      ? [{ title: t.title, layout: t.layout }]
      : t.kind === "ssh"
        ? [{ title: t.title, layout: { t: "pane" as const, id: t.id, target: { kind: "ssh" as const, hostId: t.hostId } } }]
        : t.kind === "local"
          ? [{ title: t.title, layout: { t: "pane" as const, id: t.id, target: { kind: "local" as const } } }]
          : [],
  );
  if (!terms.length) return toast("Open some terminals first; a workspace saves their layout.", "info");
  try {
    await api.workspaceSave({ id: "", name, layout: { tabs: terms } });
    await refreshLibrary();
    toast(`Saved workspace "${name}" (${terms.length} tab${terms.length === 1 ? "" : "s"})`, "success");
  } catch (e) {
    error(e);
  }
}

export function CommandsView() {
  const { library, hosts, refreshLibrary, error, setRunDialog } = useApp();
  const [q, setQ] = useState("");
  const [editing, setEditing] = useState<SavedCommand | null>(null);
  const [wsName, setWsName] = useState("");
  const shown = useMemo(() => {
    const ql = q.toLowerCase();
    return library.commands.filter((c) => !ql || [c.name, c.command, c.description].some((s) => s.toLowerCase().includes(ql)));
  }, [library.commands, q]);
  const scopeLabel = (c: SavedCommand) => {
    const hs = c.scope.hosts.map((id) => hosts.find((h) => h.id === id)?.name).filter(Boolean);
    const parts = [...hs, ...c.scope.groups.map((g) => `group ${g}`)];
    return parts.length ? parts.join(", ") : "All hosts";
  };

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto max-w-4xl space-y-8 p-6">
        <section className="space-y-3">
          <div className="flex items-center gap-2">
            <h2 className="flex-1 text-sm font-semibold">Saved commands</h2>
            <div className="relative w-56">
              <Search size={13} className="absolute left-2 top-2.5 text-fg-subtle" />
              <Input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Filter" className="h-8 pl-7" />
            </div>
            <Button size="sm" variant="primary" onClick={() => setEditing(blank())}>
              <Plus size={13} /> New command
            </Button>
          </div>
          <p className="text-xs text-fg-subtle">
            Use <code className="rounded bg-surface-3 px-1">{"{{name}}"}</code> or <code className="rounded bg-surface-3 px-1">{"{{name:default}}"}</code> for values you fill in each time;{" "}
            <code className="rounded bg-surface-3 px-1">{"{{host}}"}</code> and <code className="rounded bg-surface-3 px-1">{"{{user}}"}</code> come from the host. Pasting never presses Enter; running on hosts asks first.
          </p>
          {shown.length === 0 ? (
            <Empty icon={<ScrollText size={28} />} title={library.commands.length ? "No matches" : "No saved commands yet"}>
              Save commands you type often. Find them again with Ctrl+Shift+P, paste them into a terminal, or run them on several hosts at once.
            </Empty>
          ) : (
            <div className="divide-y divide-border rounded-lg border border-border">
              {shown.map((c) => (
                <div key={c.id} className="group flex items-start gap-3 px-3 py-2.5">
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2 text-sm">
                      <span className="font-medium">{c.name}</span>
                      {c.runOnConnect && <span className="rounded bg-accent/15 px-1.5 text-[10px] text-accent">on connect</span>}
                      <span className="truncate text-xs text-fg-subtle">{scopeLabel(c)}</span>
                    </div>
                    <pre className="mt-1 truncate font-mono text-xs text-fg-muted">{c.command}</pre>
                    {c.description && <div className="mt-0.5 text-xs text-fg-subtle">{c.description}</div>}
                  </div>
                  <div className="flex shrink-0 gap-0.5 opacity-60 group-hover:opacity-100">
                    <IconButton title="Paste into the active terminal" onClick={() => pasteCommand(c)}>
                      <SquareTerminal size={14} />
                    </IconButton>
                    <IconButton title="Run on hosts…" onClick={() => setRunDialog({ command: c.command, hostIds: c.scope.hosts })}>
                      <Play size={14} />
                    </IconButton>
                    <IconButton title="Edit" onClick={() => setEditing(c)}>
                      <Pencil size={14} />
                    </IconButton>
                    <IconButton
                      title="Delete"
                      onClick={async () => {
                        if (!(await ask(`Delete "${c.name}"?`, { title: "Delete command", kind: "warning", okLabel: "Delete" }))) return;
                        await api.commandDelete(c.id).catch(error);
                        refreshLibrary();
                      }}
                    >
                      <Trash2 size={14} />
                    </IconButton>
                  </div>
                </div>
              ))}
            </div>
          )}
        </section>

        <section className="space-y-3">
          <h2 className="text-sm font-semibold">Workspaces</h2>
          <p className="text-xs text-fg-subtle">A workspace remembers your open terminal tabs and how they are split. Open it again from here or the palette.</p>
          <div className="flex gap-2">
            <Input value={wsName} onChange={(e) => setWsName(e.target.value)} placeholder="Name, e.g. Web servers" className="max-w-xs" onKeyDown={(e) => e.key === "Enter" && wsName.trim() && (saveWorkspace(wsName.trim()), setWsName(""))} />
            <Button onClick={() => (saveWorkspace(wsName.trim()), setWsName(""))} disabled={!wsName.trim()}>
              <LayoutGrid size={14} /> Save open terminals
            </Button>
          </div>
          {library.workspaces.length > 0 && (
            <div className="divide-y divide-border rounded-lg border border-border">
              {library.workspaces.map((w) => (
                <div key={w.id} className="flex items-center gap-3 px-3 py-2 text-sm">
                  <LayoutGrid size={14} className="text-fg-subtle" />
                  <span className="flex-1">{w.name}</span>
                  <span className="text-xs text-fg-subtle">
                    {w.layout.tabs.length} tab{w.layout.tabs.length === 1 ? "" : "s"}, {w.layout.tabs.reduce((n, t) => n + allPanes(t.layout).length, 0)} terminals
                  </span>
                  <Button size="sm" onClick={() => openWorkspace(w.id)}>
                    Open
                  </Button>
                  <IconButton
                    title="Delete workspace"
                    onClick={async () => {
                      await api.workspaceDelete(w.id).catch(error);
                      refreshLibrary();
                    }}
                  >
                    <Trash2 size={14} />
                  </IconButton>
                </div>
              ))}
            </div>
          )}
        </section>
      </div>
      {editing && <CommandEditor value={editing} onClose={() => setEditing(null)} />}
    </div>
  );
}

function CommandEditor({ value, onClose }: { value: SavedCommand; onClose: () => void }) {
  const { hosts, refreshLibrary, toast, error } = useApp();
  const [c, setC] = useState<SavedCommand>(value);
  const groups = [...new Set(hosts.map((h) => h.group).filter(Boolean))];
  const vars = commandVariables(c.command);
  const scopeMode = c.scope.hosts.length ? "hosts" : c.scope.groups.length ? "groups" : "all";
  const save = async () => {
    try {
      await api.commandSave(c);
      await refreshLibrary();
      toast("Command saved", "success");
      onClose();
    } catch (e) {
      error(e);
    }
  };
  return (
    <Modal
      title={value.id ? `Edit ${value.name}` : "New command"}
      onClose={onClose}
      width="max-w-2xl"
      footer={
        <Button variant="primary" onClick={save} disabled={!c.command.trim()}>
          Save
        </Button>
      }
    >
      <div className="space-y-4">
        <Field label="Name">
          <Input autoFocus value={c.name} onChange={(e) => setC({ ...c, name: e.target.value })} placeholder="Restart a service" />
        </Field>
        <Field label="Command" hint={vars.length ? `You'll be asked for: ${vars.map((v) => v.name + (v.def ? ` (default ${v.def})` : "")).join(", ")}` : "Several lines are fine."}>
          <TextArea rows={4} value={c.command} onChange={(e) => setC({ ...c, command: e.target.value })} placeholder="sudo systemctl restart {{service:nginx}}" spellCheck={false} />
        </Field>
        <Field label="Description (optional)">
          <Input value={c.description} onChange={(e) => setC({ ...c, description: e.target.value })} />
        </Field>
        <Field label="Offer it on">
          <Select
            value={scopeMode}
            onChange={(e) => setC({ ...c, scope: { hosts: e.target.value === "hosts" ? hosts.slice(0, 1).map((h) => h.id) : [], groups: e.target.value === "groups" ? groups.slice(0, 1) : [] } })}
          >
            <option value="all">All hosts</option>
            <option value="hosts">Chosen hosts</option>
            <option value="groups" disabled={!groups.length}>
              Hosts in chosen groups
            </option>
          </Select>
        </Field>
        {scopeMode !== "all" && (
          <div className="flex max-h-40 flex-wrap gap-x-4 gap-y-1.5 overflow-y-auto rounded-md border border-border p-2.5">
            {(scopeMode === "hosts" ? hosts.map((h) => ({ k: h.id, label: h.name })) : groups.map((g) => ({ k: g, label: g }))).map((o) => {
              const list = scopeMode === "hosts" ? c.scope.hosts : c.scope.groups;
              const on = list.includes(o.k);
              const next = on ? list.filter((x) => x !== o.k) : [...list, o.k];
              return <Checkbox key={o.k} label={o.label} checked={on} onChange={() => setC({ ...c, scope: scopeMode === "hosts" ? { hosts: next, groups: [] } : { hosts: [], groups: next } })} />;
            })}
          </div>
        )}
        <div className="flex flex-col gap-1">
          <Checkbox label="Run in new terminals on these hosts" checked={c.runOnConnect} onChange={(v) => setC({ ...c, runOnConnect: v })} />
          <span className="pl-6 text-xs text-fg-subtle">Typed once after login in each new terminal (not after a reconnect, and not on tmux hosts, whose session already ran it). Variables use their defaults.</span>
        </div>
      </div>
    </Modal>
  );
}

/** The dialog that asks for a command's variables before it is used (mounted once, in App). */
export function VariablesPrompt() {
  const [req, setReq] = useState<{ cmd: SavedCommand; host: Host | null; resolve: (v: string | null) => void } | null>(null);
  const [vals, setVals] = useState<Record<string, string>>({});
  askVars = (cmd, host) =>
    new Promise((resolve) => {
      setVals(Object.fromEntries(commandVariables(cmd.command).map((v) => [v.name, v.def])));
      setReq({ cmd, host, resolve });
    });
  if (!req) return null;
  const vars = commandVariables(req.cmd.command).filter((v) => !(req.host && ["host", "user", "name"].includes(v.name)));
  const done = (ok: boolean) => {
    req.resolve(ok ? fillCommand(req.cmd.command, vals, req.host) : null);
    setReq(null);
  };
  return (
    <Modal
      title={req.cmd.name}
      onClose={() => done(false)}
      footer={
        <Button variant="primary" onClick={() => done(true)}>
          Use command
        </Button>
      }
    >
      <form
        className="space-y-3"
        onSubmit={(e) => {
          e.preventDefault();
          done(true);
        }}
      >
        {vars.map((v, i) => (
          <Field key={v.name} label={v.name}>
            <Input autoFocus={i === 0} value={vals[v.name] ?? ""} onChange={(e) => setVals({ ...vals, [v.name]: e.target.value })} />
          </Field>
        ))}
        <pre className="whitespace-pre-wrap break-all rounded-md bg-surface-0 p-2.5 font-mono text-xs text-fg-muted">{fillCommand(req.cmd.command, vals, req.host)}</pre>
        <button type="submit" hidden />
      </form>
    </Modal>
  );
}

export { commandAppliesTo };

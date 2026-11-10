import clsx from "clsx";
import { ArrowLeftRight, Box, FileUp, FolderOpen, Pencil, Plus, Search, Server, SquareTerminal, Terminal, Unplug } from "lucide-react";
import { useMemo, useState } from "react";
import { api } from "../lib/ipc";
import type { Host } from "../lib/types";
import { useApp } from "../store";
import { IconButton, Input } from "./ui";

export function openSsh(h: Host) {
  useApp.getState().openTab({ kind: "ssh", title: h.name, hostId: h.id });
}
export function openSftp(h: Host) {
  useApp.getState().openTab({ kind: "sftp", title: `${h.name} · files`, hostId: h.id }, { unique: true });
}
export function openDocker(endpoint: string, name: string) {
  useApp.getState().openTab({ kind: "docker", title: `${name} · docker`, endpoint }, { unique: true });
}

function authLabel(h: Host) {
  switch (h.auth.type) {
    case "password":
      return "password";
    case "vault_key":
      return "vault key";
    case "key_file":
      return "key file";
    case "agent":
      return "agent";
  }
}

export function HostsSidebar() {
  const { hosts, setHostEditor, setImportOpen, openTab } = useApp();
  const [q, setQ] = useState("");
  const groups = useMemo(() => {
    const ql = q.toLowerCase();
    const m = new Map<string, Host[]>();
    for (const h of hosts) {
      if (ql && ![h.name, h.hostname, h.username, h.group, ...h.tags].some((s) => s?.toLowerCase().includes(ql))) continue;
      const g = h.group || "Hosts";
      m.set(g, [...(m.get(g) ?? []), h]);
    }
    return [...m.entries()];
  }, [hosts, q]);

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between px-3 pb-2 pt-3">
        <span className="text-xs font-semibold uppercase tracking-wider text-fg-subtle">Hosts</span>
        <div className="flex">
          <IconButton title="Import from ~/.ssh/config" onClick={() => setImportOpen(true)}>
            <FileUp size={15} />
          </IconButton>
          <IconButton title="New host" onClick={() => setHostEditor("new")}>
            <Plus size={16} />
          </IconButton>
        </div>
      </div>
      <div className="px-3 pb-2">
        <div className="relative">
          <Search size={14} className="absolute left-2 top-2 text-fg-subtle" />
          <Input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Filter hosts" className="pl-7" />
        </div>
      </div>
      <button
        onClick={() => openTab({ kind: "local", title: "Local" })}
        className="mx-2 mb-2 flex items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm text-fg-muted hover:bg-surface-3 hover:text-fg cursor-pointer"
      >
        <Terminal size={15} /> Local terminal
      </button>
      <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-3">
        {groups.length === 0 && (
          <div className="px-2 py-6 text-center text-sm text-fg-subtle">
            {hosts.length === 0 ? "No hosts yet. Add one or import your ~/.ssh/config." : "No matches"}
          </div>
        )}
        {groups.map(([g, list]) => (
          <div key={g} className="mb-2">
            <div className="px-2 py-1 text-[11px] font-semibold uppercase tracking-wider text-fg-subtle">{g}</div>
            {list.map((h) => (
              <HostRow key={h.id} h={h} />
            ))}
          </div>
        ))}
      </div>
    </div>
  );
}

function HostRow({ h }: { h: Host }) {
  const { setHostEditor, toast, error, setForwardsHost } = useApp();
  const activeFwd = useApp((st) => st.forwards.filter((f) => f.hostId === h.id).length);
  return (
    <div
      className="group flex select-none items-center gap-2 rounded-md px-2 py-1.5 hover:bg-surface-3 cursor-pointer"
      onDoubleClick={() => openSsh(h)}
      title="Double-click to connect"
    >
      <div
        className={clsx("flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-surface-3 text-fg-muted")}
        style={h.color ? { background: h.color + "33", color: h.color } : undefined}
      >
        <Server size={14} />
      </div>
      <div className="min-w-0 flex-1">
        <div className="truncate text-sm">{h.name}</div>
        <div className="truncate text-[11px] text-fg-subtle">
          {h.username}@{h.hostname}
          {h.port !== 22 ? `:${h.port}` : ""} · {authLabel(h)}
          {activeFwd > 0 && <span className="ml-1 text-ok">· {activeFwd} fwd</span>}
        </div>
      </div>
      <div className="hidden shrink-0 group-hover:flex">
        <IconButton title="Open terminal" onClick={() => openSsh(h)}>
          <SquareTerminal size={14} />
        </IconButton>
        <IconButton title="Files (SFTP)" onClick={() => openSftp(h)}>
          <FolderOpen size={14} />
        </IconButton>
        <IconButton title="Port forwards" onClick={() => setForwardsHost(h.id)}>
          <ArrowLeftRight size={14} />
        </IconButton>
        <IconButton title="Docker" onClick={() => openDocker(h.id, h.name)}>
          <Box size={14} />
        </IconButton>
        <IconButton
          title="Disconnect"
          onClick={() =>
            api
              .hostDisconnect(h.id)
              .then(() => toast(`Disconnected ${h.name}`))
              .catch(error)
          }
        >
          <Unplug size={14} />
        </IconButton>
        <IconButton title="Edit" onClick={() => setHostEditor(h)}>
          <Pencil size={14} />
        </IconButton>
      </div>
    </div>
  );
}

export function DockerSidebar() {
  const { hosts } = useApp();
  return (
    <div className="flex h-full flex-col">
      <div className="px-3 pb-2 pt-3 text-xs font-semibold uppercase tracking-wider text-fg-subtle">Docker endpoints</div>
      <div className="min-h-0 flex-1 overflow-y-auto px-2">
        <EndpointRow name="Local" detail="unix:///var/run/docker.sock" onClick={() => openDocker("local", "Local")} />
        <div className="px-2 pb-1 pt-3 text-[11px] font-semibold uppercase tracking-wider text-fg-subtle">Over SSH</div>
        {hosts.length === 0 && <div className="px-2 text-sm text-fg-subtle">Add an SSH host to manage its containers.</div>}
        {hosts.map((h) => (
          <EndpointRow key={h.id} name={h.name} detail={`${h.hostname} · ${h.dockerSocket || "/var/run/docker.sock"}`} onClick={() => openDocker(h.id, h.name)} />
        ))}
      </div>
    </div>
  );
}

function EndpointRow({ name, detail, onClick }: { name: string; detail: string; onClick: () => void }) {
  return (
    <button onClick={onClick} className="flex w-full select-none items-center gap-2 rounded-md px-2 py-1.5 text-left hover:bg-surface-3 cursor-pointer">
      <div className="flex h-7 w-7 items-center justify-center rounded-md bg-accent/15 text-accent">
        <Box size={14} />
      </div>
      <div className="min-w-0">
        <div className="truncate text-sm">{name}</div>
        <div className="truncate text-[11px] text-fg-subtle">{detail}</div>
      </div>
    </button>
  );
}

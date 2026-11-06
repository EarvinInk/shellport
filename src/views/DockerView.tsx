import clsx from "clsx";
import { Box, FileText, Info, Pause, Play, RefreshCw, RotateCw, Skull, Square, SquareTerminal, Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { TerminalView } from "../components/TerminalView";
import { Badge, Button, Checkbox, Empty, IconButton, Input, Modal } from "../components/ui";
import { api, errMsg, formatBytes } from "../lib/ipc";
import type { ContainerInfo, DockerAction, EndpointInfo, ImageInfo } from "../lib/types";
import { useApp } from "../store";

function stateTone(s: string): "ok" | "warn" | "bad" | "neutral" {
  if (s === "running") return "ok";
  if (s === "paused" || s === "restarting" || s === "created") return "warn";
  if (s === "dead" || s === "exited") return "bad";
  return "neutral";
}

export function DockerView({ endpoint, active }: { endpoint: string; active: boolean }) {
  const { openTab, error, toast } = useApp();
  const [info, setInfo] = useState<EndpointInfo | null>(null);
  const [containers, setContainers] = useState<ContainerInfo[]>([]);
  const [images, setImages] = useState<ImageInfo[]>([]);
  const [tab, setTab] = useState<"containers" | "images">("containers");
  const [all, setAll] = useState(true);
  const [auto, setAuto] = useState(true);
  const [q, setQ] = useState("");
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState<Record<string, string>>({});
  const [inspect, setInspect] = useState<{ name: string; json: unknown } | null>(null);
  const [confirmRm, setConfirmRm] = useState<ContainerInfo | null>(null);
  const endpointName = useApp((s) => (endpoint === "local" ? "Local" : s.hosts.find((h) => h.id === endpoint)?.name ?? endpoint));

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      if (!info) setInfo(await api.dockerInfo(endpoint));
      if (tab === "containers") setContainers(await api.dockerList(endpoint, all));
      else setImages(await api.dockerImages(endpoint));
      setErr(null);
    } catch (e) {
      setErr(errMsg(e));
    } finally {
      setLoading(false);
    }
  }, [endpoint, all, tab, info]);

  useEffect(() => {
    if (active) refresh();
  }, [active, all, tab]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!active || !auto || err) return;
    const t = setInterval(refresh, 5000);
    return () => clearInterval(t);
  }, [active, auto, refresh, err]);

  const act = async (c: ContainerInfo, a: DockerAction) => {
    setBusy((b) => ({ ...b, [c.id]: a }));
    try {
      await api.dockerAction(endpoint, c.id, a);
      toast(`${a} ${c.name}: ok`, "success");
      await refresh();
    } catch (e) {
      error(e);
    } finally {
      setBusy(({ [c.id]: _, ...rest }) => rest);
    }
  };

  const grouped = useMemo(() => {
    const ql = q.toLowerCase();
    const list = containers.filter((c) => !ql || [c.name, c.image, c.project ?? "", c.id].some((s) => s.toLowerCase().includes(ql)));
    const m = new Map<string, ContainerInfo[]>();
    for (const c of list) {
      const k = c.project ?? "";
      m.set(k, [...(m.get(k) ?? []), c]);
    }
    return [...m.entries()].sort(([a], [b]) => (a === "" ? 1 : b === "" ? -1 : a.localeCompare(b)));
  }, [containers, q]);

  const running = containers.filter((c) => c.state === "running").length;

  return (
    <div className="flex h-full flex-col">
      <div className="flex flex-wrap items-center gap-2 border-b border-border px-4 py-2.5">
        <Box className="text-sky-400" size={18} />
        <span className="font-semibold">{endpointName}</span>
        {info && (
          <>
            <Badge tone="info">{info.mode === "local" ? "local socket" : info.mode === "ssh-socket" ? "SSH socket tunnel" : "SSH docker CLI"}</Badge>
            {info.serverVersion && <Badge>Docker {info.serverVersion}</Badge>}
          </>
        )}
        <Badge tone="ok">{running} running</Badge>
        <div className="ml-3 flex rounded-md bg-surface-2 p-0.5 text-sm">
          {(["containers", "images"] as const).map((t) => (
            <button key={t} onClick={() => setTab(t)} className={clsx("rounded px-3 py-1 capitalize cursor-pointer", tab === t ? "bg-surface-4 text-fg" : "text-fg-muted")}>
              {t}
            </button>
          ))}
        </div>
        <div className="flex-1" />
        <Input className="w-56" placeholder="Filter" value={q} onChange={(e) => setQ(e.target.value)} />
        {tab === "containers" && <Checkbox label="Show stopped" checked={all} onChange={setAll} />}
        <Checkbox label="Auto refresh" checked={auto} onChange={setAuto} />
        <IconButton title="Refresh" onClick={refresh}>
          <RefreshCw size={15} className={loading ? "animate-spin" : ""} />
        </IconButton>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {err ? (
          <Empty icon={<Box size={32} />} title="Docker unavailable">
            <div className="mb-3 whitespace-pre-wrap">{err}</div>
            <Button
              onClick={async () => {
                await api.dockerReset(endpoint);
                setInfo(null);
                setErr(null);
                refresh();
              }}
            >
              Retry
            </Button>
          </Empty>
        ) : tab === "containers" ? (
          grouped.length === 0 ? (
            <Empty icon={<Box size={32} />} title={loading ? "Loading…" : "No containers"} />
          ) : (
            <table className="w-full text-sm">
              <thead className="sticky top-0 z-10 bg-surface-0 text-left text-[11px] uppercase text-fg-subtle">
                <tr>
                  <th className="px-4 py-2 font-medium">Name</th>
                  <th className="px-2 font-medium">Image</th>
                  <th className="px-2 font-medium">State</th>
                  <th className="px-2 font-medium">Ports</th>
                  <th className="px-2 text-right font-medium">Actions</th>
                </tr>
              </thead>
              <tbody>
                {grouped.map(([project, list]) => (
                  <ProjectGroup key={project || "_"} project={project}>
                    {list.map((c) => {
                      const isRunning = c.state === "running";
                      const isPaused = c.state === "paused";
                      const b = busy[c.id];
                      return (
                        <tr key={c.id} className="border-t border-border/60 hover:bg-surface-2">
                          <td className="px-4 py-1.5">
                            <div className="font-medium">{c.name}</div>
                            <div className="font-mono text-[11px] text-fg-subtle">{c.id.slice(0, 12)}</div>
                          </td>
                          <td className="max-w-64 truncate px-2 text-fg-muted" title={c.image}>
                            {c.image}
                          </td>
                          <td className="px-2">
                            <Badge tone={stateTone(c.state)}>{b ? `${b}…` : c.state}</Badge>
                            <div className="text-[11px] text-fg-subtle">{c.status}</div>
                          </td>
                          <td className="max-w-56 px-2 font-mono text-[11px] text-fg-muted">{c.ports.join(", ")}</td>
                          <td className="px-2">
                            <div className="flex justify-end">
                              {!isRunning && !isPaused && (
                                <IconButton title="Start" disabled={!!b} onClick={() => act(c, "start")}>
                                  <Play size={14} className="text-ok" />
                                </IconButton>
                              )}
                              {isRunning && (
                                <IconButton title="Stop" disabled={!!b} onClick={() => act(c, "stop")}>
                                  <Square size={14} />
                                </IconButton>
                              )}
                              {(isRunning || isPaused) && (
                                <IconButton title="Restart" disabled={!!b} onClick={() => act(c, "restart")}>
                                  <RotateCw size={14} />
                                </IconButton>
                              )}
                              {isRunning && (
                                <IconButton title="Pause" disabled={!!b} onClick={() => act(c, "pause")}>
                                  <Pause size={14} />
                                </IconButton>
                              )}
                              {isPaused && (
                                <IconButton title="Unpause" disabled={!!b} onClick={() => act(c, "unpause")}>
                                  <Play size={14} />
                                </IconButton>
                              )}
                              <IconButton
                                title="Logs"
                                onClick={() => openTab({ kind: "logs", title: `logs · ${c.name}`, endpoint, containerId: c.id }, { unique: true })}
                              >
                                <FileText size={14} />
                              </IconButton>
                              <IconButton
                                title="Shell"
                                disabled={!isRunning}
                                onClick={() => openTab({ kind: "exec", title: `${c.name} · sh`, endpoint, containerId: c.id })}
                              >
                                <SquareTerminal size={14} />
                              </IconButton>
                              <IconButton
                                title="Inspect"
                                onClick={async () => {
                                  try {
                                    setInspect({ name: c.name, json: await api.dockerInspect(endpoint, c.id) });
                                  } catch (e) {
                                    error(e);
                                  }
                                }}
                              >
                                <Info size={14} />
                              </IconButton>
                              {isRunning && (
                                <IconButton title="Kill" disabled={!!b} onClick={() => act(c, "kill")}>
                                  <Skull size={14} />
                                </IconButton>
                              )}
                              <IconButton title="Remove" disabled={!!b} onClick={() => setConfirmRm(c)}>
                                <Trash2 size={14} className="text-danger" />
                              </IconButton>
                            </div>
                          </td>
                        </tr>
                      );
                    })}
                  </ProjectGroup>
                ))}
              </tbody>
            </table>
          )
        ) : (
          <table className="w-full text-sm">
            <thead className="sticky top-0 bg-surface-0 text-left text-[11px] uppercase text-fg-subtle">
              <tr>
                <th className="px-4 py-2 font-medium">Repository:tag</th>
                <th className="px-2 font-medium">ID</th>
                <th className="px-2 font-medium">Size</th>
                <th className="px-2 font-medium">Created</th>
              </tr>
            </thead>
            <tbody>
              {images
                .filter((i) => !q || i.tags.join(" ").toLowerCase().includes(q.toLowerCase()))
                .map((i) => (
                  <tr key={i.id} className="border-t border-border/60 hover:bg-surface-2">
                    <td className="px-4 py-1.5">{i.tags.length ? i.tags.join(", ") : <span className="text-fg-subtle">&lt;none&gt;</span>}</td>
                    <td className="px-2 font-mono text-xs text-fg-muted">{i.id.replace("sha256:", "").slice(0, 12)}</td>
                    <td className="px-2 text-fg-muted">{i.size ? formatBytes(i.size) : ""}</td>
                    <td className="px-2 text-fg-muted">{i.created ? new Date(i.created * 1000).toLocaleDateString() : ""}</td>
                  </tr>
                ))}
            </tbody>
          </table>
        )}
      </div>
      {inspect && (
        <Modal title={`Inspect ${inspect.name}`} onClose={() => setInspect(null)} width="max-w-4xl">
          <pre className="whitespace-pre-wrap break-all font-mono text-xs">{JSON.stringify(inspect.json, null, 2)}</pre>
        </Modal>
      )}
      {confirmRm && (
        <Modal
          title="Remove container"
          onClose={() => setConfirmRm(null)}
          width="max-w-sm"
          footer={
            <Button
              variant="danger"
              onClick={() => {
                act(confirmRm, "remove");
                setConfirmRm(null);
              }}
            >
              Force remove
            </Button>
          }
        >
          <p className="text-sm">
            Force-remove <b>{confirmRm.name}</b>? Running containers are killed first. Volumes are kept.
          </p>
        </Modal>
      )}
    </div>
  );
}

function ProjectGroup({ project, children }: { project: string; children: React.ReactNode }) {
  return (
    <>
      {project && (
        <tr>
          <td colSpan={5} className="bg-surface-1 px-4 py-1 text-[11px] font-semibold uppercase tracking-wider text-fg-subtle">
            compose · {project}
          </td>
        </tr>
      )}
      {children}
    </>
  );
}

export function LogsView({ endpoint, containerId, active }: { endpoint: string; containerId: string; active: boolean }) {
  const settings = useApp((s) => s.settings);
  const [tail, setTail] = useState(settings?.dockerLogTail ?? 500);
  const [timestamps, setTimestamps] = useState(false);
  const [follow, setFollow] = useState(true);
  const [gen, setGen] = useState(0);
  const restart = () => setGen((g) => g + 1);
  return (
    <TerminalView
      active={active}
      readOnly
      generation={gen}
      open={(_c, _r, ch) => api.dockerLogs(endpoint, containerId, tail, timestamps, follow, ch)}
      toolbar={
        <div className="flex items-center gap-3 border-b border-border px-3 py-1.5 text-sm">
          <FileText size={15} className="text-fg-subtle" />
          <label className="flex items-center gap-1 text-fg-muted">
            tail
            <Input type="number" className="h-7 w-20" value={tail} onChange={(e) => setTail(Number(e.target.value) || 0)} />
          </label>
          <Checkbox label="Follow" checked={follow} onChange={setFollow} />
          <Checkbox label="Timestamps" checked={timestamps} onChange={setTimestamps} />
          <Button size="sm" onClick={restart}>
            <RotateCw size={13} /> Apply / reload
          </Button>
          <span className="text-xs text-fg-subtle">Ctrl+Shift+F to search</span>
        </div>
      }
    />
  );
}

export function ExecView({ endpoint, containerId, active, onTitle }: { endpoint: string; containerId: string; active: boolean; onTitle: (t: string) => void }) {
  return <TerminalView active={active} onTitle={onTitle} open={(c, r, ch) => api.dockerExec(endpoint, containerId, c, r, ch)} />;
}

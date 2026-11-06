import clsx from "clsx";
import {
  Box,
  ChevronDown,
  ChevronRight,
  Copy,
  Download,
  FileText,
  Info,
  Layers,
  Loader2,
  MoreHorizontal,
  Pause,
  Play,
  RefreshCw,
  RotateCw,
  Search,
  Skull,
  Square,
  SquareTerminal,
  Trash2,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { TerminalView } from "../components/TerminalView";
import { Badge, Button, Checkbox, Empty, IconButton, Input, Modal } from "../components/ui";
import { api, errMsg, formatBytes } from "../lib/ipc";
import type { ContainerInfo, DockerAction, EndpointInfo, ImageInfo } from "../lib/types";
import { useApp } from "../store";
import { MultiLogsView, NetworksPanel, PullDialog, SystemPanel, VolumesPanel } from "./DockerAdmin";

export { MultiLogsView };

type DockerTab = "containers" | "images" | "volumes" | "networks" | "system";

type Tone = "ok" | "warn" | "bad" | "neutral";

function stateTone(s: string): Tone {
  if (s === "running") return "ok";
  if (s === "paused" || s === "restarting" || s === "created") return "warn";
  if (s === "dead" || (s === "exited")) return "bad";
  return "neutral";
}

const DOT: Record<Tone, string> = {
  ok: "bg-ok shadow-[0_0_0_3px] shadow-ok/20",
  warn: "bg-warn shadow-[0_0_0_3px] shadow-warn/20",
  bad: "bg-danger/80",
  neutral: "bg-fg-subtle",
};

/** "0.0.0.0:5434->5432/tcp, :::5434->5432/tcp" → ["5434→5432"] (IPv4/IPv6 duplicates merged). */
function tidyPorts(ports: string[]): { label: string; published: boolean; local: boolean }[] {
  const seen = new Map<string, { label: string; published: boolean; local: boolean }>();
  for (const raw of ports.flatMap((p) => p.split(",")).map((p) => p.trim()).filter(Boolean)) {
    const [left, right] = raw.split("->");
    if (right === undefined) {
      const label = left.replace(/\/tcp$/, "");
      if (!seen.has(label)) seen.set(label, { label, published: false, local: false });
      continue;
    }
    const i = left.lastIndexOf(":");
    const ip = i >= 0 ? left.slice(0, i) : "";
    const pub = i >= 0 ? left.slice(i + 1) : left;
    const label = `${pub}→${right.replace(/\/tcp$/, "")}`;
    const local = ip.startsWith("127.") || ip === "::1";
    const prev = seen.get(label);
    seen.set(label, { label, published: true, local: prev ? prev.local && local : local });
  }
  return [...seen.values()];
}

function PortChips({ ports }: { ports: string[] }) {
  const list = tidyPorts(ports);
  if (list.length === 0) return <span className="text-fg-subtle">—</span>;
  const shown = list.slice(0, 3);
  return (
    <div className="flex flex-wrap gap-1" title={ports.join("\n")}>
      {shown.map((p) => (
        <span
          key={p.label}
          className={clsx(
            "rounded px-1.5 py-0.5 font-mono text-[11px] leading-none",
            p.published ? "bg-sky-500/10 text-sky-300" : "bg-surface-3 text-fg-subtle",
          )}
        >
          {p.label}
          {p.local && <span className="ml-1 text-fg-subtle">local</span>}
        </span>
      ))}
      {list.length > shown.length && <span className="px-1 text-[11px] text-fg-subtle">+{list.length - shown.length}</span>}
    </div>
  );
}

function Menu({ items }: { items: { label: string; icon: React.ReactNode; onClick: () => void; danger?: boolean; disabled?: boolean }[] }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const h = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setOpen(false);
    window.addEventListener("mousedown", h);
    return () => window.removeEventListener("mousedown", h);
  }, [open]);
  return (
    <div className="relative" ref={ref}>
      <IconButton title="More actions" onClick={() => setOpen((o) => !o)} className={open ? "bg-surface-3 text-fg" : ""}>
        <MoreHorizontal size={15} />
      </IconButton>
      {open && (
        <div className="absolute right-0 top-8 z-20 w-44 overflow-hidden rounded-lg border border-border bg-surface-2 py-1 shadow-2xl">
          {items.map((it) => (
            <button
              key={it.label}
              disabled={it.disabled}
              onClick={() => {
                setOpen(false);
                it.onClick();
              }}
              className={clsx(
                "flex w-full items-center gap-2.5 px-3 py-1.5 text-left text-sm disabled:opacity-40 cursor-pointer",
                it.danger ? "text-danger hover:bg-danger/10" : "text-fg hover:bg-surface-3",
              )}
            >
              <span className="text-fg-muted">{it.icon}</span>
              {it.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

export function DockerView({ endpoint, active }: { endpoint: string; active: boolean }) {
  const { openTab, error, toast } = useApp();
  const [info, setInfo] = useState<EndpointInfo | null>(null);
  const [containers, setContainers] = useState<ContainerInfo[]>([]);
  const [images, setImages] = useState<ImageInfo[]>([]);
  const [tab, setTab] = useState<DockerTab>("containers");
  const [pulling, setPulling] = useState(false);
  const [rmImage, setRmImage] = useState<ImageInfo | null>(null);
  const [projectBusy, setProjectBusy] = useState<Record<string, string>>({});
  const [all, setAll] = useState(true);
  const [auto, setAuto] = useState(true);
  const [q, setQ] = useState("");
  const [loading, setLoading] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState<Record<string, string>>({});
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [inspect, setInspect] = useState<{ name: string; json: unknown } | null>(null);
  const [confirmRm, setConfirmRm] = useState<ContainerInfo | null>(null);
  const endpointName = useApp((s) => (endpoint === "local" ? "Local" : s.hosts.find((h) => h.id === endpoint)?.name ?? endpoint));

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      if (!info) setInfo(await api.dockerInfo(endpoint));
      if (tab === "containers") setContainers(await api.dockerList(endpoint, all));
      else if (tab === "images") setImages(await api.dockerImages(endpoint));
      setErr(null);
      setLoaded(true);
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
      toast(`${c.name}: ${a} ok`, "success");
      await refresh();
    } catch (e) {
      error(e);
    } finally {
      setBusy(({ [c.id]: _, ...rest }) => rest);
    }
  };

  const projectAct = async (project: string, a: DockerAction) => {
    setProjectBusy((b) => ({ ...b, [project]: a }));
    try {
      const n = await api.dockerProjectAction(endpoint, project, a);
      toast(`${project}: ${a} ${n} container${n === 1 ? "" : "s"}`, "success");
      await refresh();
    } catch (e) {
      error(e);
    } finally {
      setProjectBusy(({ [project]: _, ...rest }) => rest);
    }
  };

  const removeImage = async (img: ImageInfo, force: boolean) => {
    const ref = img.tags.find((t) => t !== "<none>:<none>") ?? img.id;
    try {
      await api.dockerImageRemove(endpoint, ref, force);
      toast(`Removed ${ref}`, "success");
      setRmImage(null);
      await refresh();
    } catch (e) {
      error(e);
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
  const stopped = containers.length - running;
  const filteredImages = images.filter((i) => !q || i.tags.join(" ").toLowerCase().includes(q.toLowerCase()));

  const toggleGroup = (g: string) =>
    setCollapsed((s) => {
      const n = new Set(s);
      if (n.has(g)) n.delete(g);
      else n.add(g);
      return n;
    });

  return (
    <div className="flex h-full flex-col">
      {/* Header */}
      <div className="border-b border-border px-5 pb-3 pt-4">
        <div className="flex items-center gap-3">
          <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-accent/15 text-accent">
            <Box size={18} />
          </div>
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <span className="text-base font-semibold">{endpointName}</span>
              {info && <Badge tone="info">{info.mode === "local" ? "local socket" : info.mode === "ssh-socket" ? "SSH tunnel" : "SSH · docker CLI"}</Badge>}
            </div>
            <div className="text-xs text-fg-subtle">
              {info?.serverVersion ? `Docker ${info.serverVersion}` : loading ? "Connecting…" : "Docker"}
              {loaded && tab === "containers" && (
                <>
                  {" · "}
                  <span className="text-ok">{running} running</span>
                  {stopped > 0 && <> · {stopped} stopped</>}
                </>
              )}
              {loaded && tab === "images" && <> · {images.length} images</>}
            </div>
          </div>
          <div className="flex-1" />
          <div className="flex rounded-lg bg-surface-2 p-0.5 text-sm">
            {(["containers", "images", "volumes", "networks", "system"] as const).map((t) => (
              <button
                key={t}
                onClick={() => setTab(t)}
                className={clsx("rounded-md px-3.5 py-1.5 capitalize cursor-pointer", tab === t ? "bg-surface-4 text-fg shadow" : "text-fg-muted hover:text-fg")}
              >
                {t}
              </button>
            ))}
          </div>
          <IconButton title="Refresh" onClick={refresh} className="h-9 w-9">
            <RefreshCw size={15} className={loading ? "animate-spin" : ""} />
          </IconButton>
        </div>
        <div className="mt-3 flex items-center gap-4">
          <div className="relative w-80">
            <Search size={14} className="absolute left-3 top-2.5 text-fg-subtle" />
            <Input className="pl-8" placeholder={tab === "containers" ? "Filter by name, image or project" : `Filter ${tab}`} value={q} onChange={(e) => setQ(e.target.value)} />
          </div>
          <div className="flex-1" />
          {tab === "containers" && containers.some((c) => c.state === "running") && (
            <Button
              size="sm"
              variant="ghost"
              onClick={() => {
                const running = containers.filter((c) => c.state === "running").map((c) => ({ id: c.id, name: c.name }));
                openTab({ kind: "mlogs", title: `logs · all running`, endpoint, containers: running });
              }}
            >
              <FileText size={13} /> Logs of all running
            </Button>
          )}
          {tab === "images" && (
            <Button size="sm" onClick={() => setPulling(true)}>
              <Download size={13} /> Pull image
            </Button>
          )}
          {tab === "containers" && <Checkbox label="Show stopped" checked={all} onChange={setAll} />}
          <Checkbox label="Auto refresh" checked={auto} onChange={setAuto} />
        </div>
      </div>

      {/* Body */}
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
        ) : tab === "volumes" ? (
          <VolumesPanel endpoint={endpoint} active={active} filter={q} />
        ) : tab === "networks" ? (
          <NetworksPanel endpoint={endpoint} active={active} filter={q} />
        ) : tab === "system" ? (
          <SystemPanel endpoint={endpoint} active={active} />
        ) : tab === "containers" ? (
          grouped.length === 0 ? (
            <Empty icon={loading ? <Loader2 size={28} className="animate-spin" /> : <Box size={32} />} title={loading ? "Loading containers…" : q ? "No matches" : "No containers"} />
          ) : (
            <table className="w-full table-fixed text-sm">
              <colgroup>
                <col />
                <col className="w-[22%]" />
                <col className="w-44" />
                <col className="w-48" />
                <col className="w-40" />
              </colgroup>
              <thead className="sticky top-0 z-10 bg-surface-0 text-left text-[11px] uppercase tracking-wider text-fg-subtle shadow-[0_1px_0_var(--color-border)]">
                <tr>
                  <th className="py-2.5 pl-5 font-medium">Container</th>
                  <th className="px-3 font-medium">Image</th>
                  <th className="px-3 font-medium">Status</th>
                  <th className="px-3 font-medium">Ports</th>
                  <th className="pr-5 text-right font-medium">Actions</th>
                </tr>
              </thead>
              <tbody>
                {grouped.map(([project, list]) => {
                  const isCollapsed = collapsed.has(project);
                  const up = list.filter((c) => c.state === "running").length;
                  return (
                    <GroupRows key={project || "_"}>
                      {project && (
                        <tr className="cursor-pointer select-none bg-surface-1 hover:bg-surface-2" onClick={() => toggleGroup(project)}>
                          <td colSpan={5} className="py-2 pl-4 pr-5">
                            <div className="flex items-center gap-2">
                              {isCollapsed ? <ChevronRight size={14} className="text-fg-subtle" /> : <ChevronDown size={14} className="text-fg-subtle" />}
                              <Layers size={13} className="text-fg-subtle" />
                              <span className="text-[13px] font-medium">{project}</span>
                              <span className={clsx("text-xs", up === list.length ? "text-ok" : up === 0 ? "text-fg-subtle" : "text-warn")}>
                                {projectBusy[project] ? `${projectBusy[project]}…` : `${up}/${list.length} running`}
                              </span>
                              <div className="flex-1" />
                              <div className="flex items-center gap-0.5" onClick={(e) => e.stopPropagation()}>
                                <Button
                                  size="sm"
                                  variant="ghost"
                                  onClick={() => openTab({ kind: "mlogs", title: `logs · ${project}`, endpoint, containers: list.map((c) => ({ id: c.id, name: c.name })) }, { unique: true })}
                                >
                                  <FileText size={13} /> Logs
                                </Button>
                                {up < list.length && (
                                  <IconButton title="Start all" disabled={!!projectBusy[project]} onClick={() => projectAct(project, "start")}>
                                    <Play size={14} className="text-ok" />
                                  </IconButton>
                                )}
                                {up > 0 && (
                                  <IconButton title="Stop all" disabled={!!projectBusy[project]} onClick={() => projectAct(project, "stop")}>
                                    <Square size={14} />
                                  </IconButton>
                                )}
                                <IconButton title="Restart all" disabled={!!projectBusy[project]} onClick={() => projectAct(project, "restart")}>
                                  <RotateCw size={14} />
                                </IconButton>
                              </div>
                            </div>
                          </td>
                        </tr>
                      )}
                      {!isCollapsed &&
                        list.map((c) => (
                          <ContainerRow
                            key={c.id}
                            c={c}
                            indent={!!project}
                            busy={busy[c.id]}
                            onAction={(a) => (a === "remove" ? setConfirmRm(c) : act(c, a))}
                            onLogs={() => openTab({ kind: "logs", title: `logs · ${c.name}`, endpoint, containerId: c.id }, { unique: true })}
                            onShell={() => openTab({ kind: "exec", title: `${c.name} · sh`, endpoint, containerId: c.id })}
                            onInspect={async () => {
                              try {
                                setInspect({ name: c.name, json: await api.dockerInspect(endpoint, c.id) });
                              } catch (e) {
                                error(e);
                              }
                            }}
                            onCopyId={() => {
                              navigator.clipboard.writeText(c.id);
                              toast("Container ID copied");
                            }}
                          />
                        ))}
                    </GroupRows>
                  );
                })}
              </tbody>
            </table>
          )
        ) : filteredImages.length === 0 ? (
          <Empty icon={<Layers size={32} />} title={loading ? "Loading images…" : "No images"} />
        ) : (
          <table className="w-full table-fixed text-sm">
            <colgroup>
              <col />
              <col className="w-36" />
              <col className="w-28" />
              <col className="w-32" />
              <col className="w-20" />
            </colgroup>
            <thead className="sticky top-0 z-10 bg-surface-0 text-left text-[11px] uppercase tracking-wider text-fg-subtle shadow-[0_1px_0_var(--color-border)]">
              <tr>
                <th className="py-2.5 pl-5 font-medium">Repository : tag</th>
                <th className="px-3 font-medium">Image ID</th>
                <th className="px-3 text-right font-medium">Size</th>
                <th className="px-3 text-right font-medium">Created</th>
                <th className="pr-5 text-right font-medium" />
              </tr>
            </thead>
            <tbody>
              {filteredImages.map((i) => (
                <tr key={i.id} className="border-b border-border/50 hover:bg-surface-2">
                  <td className="truncate py-2.5 pl-5">
                    {i.tags.length && i.tags[0] !== "<none>:<none>" ? (
                      i.tags.map((t) => {
                        const k = t.lastIndexOf(":");
                        return (
                          <span key={t} className="mr-3">
                            <span className="font-medium">{k > 0 ? t.slice(0, k) : t}</span>
                            {k > 0 && <span className="text-fg-subtle">:{t.slice(k + 1)}</span>}
                          </span>
                        );
                      })
                    ) : (
                      <span className="text-fg-subtle">&lt;untagged&gt;</span>
                    )}
                  </td>
                  <td className="px-3 font-mono text-xs text-fg-muted">{i.id.replace("sha256:", "").slice(0, 12)}</td>
                  <td className="px-3 text-right text-fg-muted">{i.size ? formatBytes(i.size) : "—"}</td>
                  <td className="px-3 text-right text-fg-muted">{i.created ? new Date(i.created * 1000).toLocaleDateString() : "—"}</td>
                  <td className="pr-5">
                    <div className="flex justify-end">
                      <IconButton title="Remove image" onClick={() => setRmImage(i)}>
                        <Trash2 size={14} className="text-danger" />
                      </IconButton>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      {pulling && <PullDialog endpoint={endpoint} onClose={() => setPulling(false)} onDone={refresh} />}
      {rmImage && (
        <Modal
          title="Remove image"
          onClose={() => setRmImage(null)}
          width="max-w-md"
          footer={
            <>
              <Button variant="ghost" onClick={() => setRmImage(null)}>
                Cancel
              </Button>
              <Button variant="danger" onClick={() => removeImage(rmImage, true)}>
                Force remove
              </Button>
              <Button variant="danger" onClick={() => removeImage(rmImage, false)}>
                <Trash2 size={14} /> Remove
              </Button>
            </>
          }
        >
          <p className="text-sm leading-relaxed">
            Remove <b className="break-all">{rmImage.tags.filter((t) => t !== "<none>:<none>").join(", ") || rmImage.id.replace("sha256:", "").slice(0, 12)}</b>? Force also removes it
            when stopped containers still reference it.
          </p>
        </Modal>
      )}
      {inspect && (
        <Modal title={`Inspect · ${inspect.name}`} onClose={() => setInspect(null)} width="max-w-4xl">
          <pre className="whitespace-pre-wrap break-all rounded-lg bg-surface-1 p-4 font-mono text-xs leading-relaxed">{JSON.stringify(inspect.json, null, 2)}</pre>
        </Modal>
      )}
      {confirmRm && (
        <Modal
          title="Remove container"
          onClose={() => setConfirmRm(null)}
          width="max-w-md"
          footer={
            <>
              <Button variant="ghost" onClick={() => setConfirmRm(null)}>
                Cancel
              </Button>
              <Button
                variant="danger"
                onClick={() => {
                  act(confirmRm, "remove");
                  setConfirmRm(null);
                }}
              >
                <Trash2 size={14} /> Force remove
              </Button>
            </>
          }
        >
          <p className="text-sm leading-relaxed">
            Force-remove <b>{confirmRm.name}</b>? A running container is killed first. Named volumes are kept.
          </p>
        </Modal>
      )}
    </div>
  );
}

function GroupRows({ children }: { children: React.ReactNode }) {
  return <>{children}</>;
}

function ContainerRow({
  c,
  indent,
  busy,
  onAction,
  onLogs,
  onShell,
  onInspect,
  onCopyId,
}: {
  c: ContainerInfo;
  indent: boolean;
  busy?: string;
  onAction: (a: DockerAction) => void;
  onLogs: () => void;
  onShell: () => void;
  onInspect: () => void;
  onCopyId: () => void;
}) {
  const isRunning = c.state === "running";
  const isPaused = c.state === "paused";
  const tone = stateTone(c.state);
  const imgTag = c.image.lastIndexOf(":") > c.image.lastIndexOf("/") ? c.image.lastIndexOf(":") : -1;
  return (
    <tr className="group border-b border-border/50 hover:bg-surface-2">
      <td className={clsx("py-2.5 pr-3", indent ? "pl-10" : "pl-5")}>
        <div className="flex items-center gap-3">
          {busy ? <Loader2 size={10} className="shrink-0 animate-spin text-fg-muted" /> : <span className={clsx("h-2 w-2 shrink-0 rounded-full", DOT[tone])} />}
          <div className="min-w-0">
            <div className="truncate font-medium" title={c.name}>
              {c.name}
            </div>
            <div className="font-mono text-[11px] text-fg-subtle">{c.id.slice(0, 12)}</div>
          </div>
        </div>
      </td>
      <td className="truncate px-3 text-fg-muted" title={c.image}>
        {imgTag > 0 ? (
          <>
            {c.image.slice(0, imgTag)}
            <span className="text-fg-subtle">{c.image.slice(imgTag)}</span>
          </>
        ) : (
          c.image
        )}
      </td>
      <td className="px-3">
        <div className={clsx("text-[13px]", tone === "ok" ? "text-fg" : tone === "bad" ? "text-danger/90" : tone === "warn" ? "text-warn" : "text-fg-muted")}>
          {busy ? `${busy}…` : c.state.charAt(0).toUpperCase() + c.state.slice(1)}
        </div>
        <div className="truncate text-[11px] text-fg-subtle" title={c.status}>
          {c.status}
        </div>
      </td>
      <td className="px-3">
        <PortChips ports={c.ports} />
      </td>
      <td className="pr-5">
        <div className="flex items-center justify-end gap-0.5">
          {isRunning || isPaused ? (
            <IconButton title="Stop" disabled={!!busy} onClick={() => onAction("stop")}>
              <Square size={14} />
            </IconButton>
          ) : (
            <IconButton title="Start" disabled={!!busy} onClick={() => onAction("start")}>
              <Play size={15} className="text-ok" />
            </IconButton>
          )}
          <IconButton title="Restart" disabled={!!busy || !(isRunning || isPaused)} onClick={() => onAction("restart")}>
            <RotateCw size={14} />
          </IconButton>
          <IconButton title="Logs" onClick={onLogs}>
            <FileText size={14} />
          </IconButton>
          <IconButton title="Open shell" disabled={!isRunning} onClick={onShell}>
            <SquareTerminal size={14} />
          </IconButton>
          <Menu
            items={[
              isPaused
                ? { label: "Unpause", icon: <Play size={14} />, onClick: () => onAction("unpause"), disabled: !!busy }
                : { label: "Pause", icon: <Pause size={14} />, onClick: () => onAction("pause"), disabled: !!busy || !isRunning },
              { label: "Inspect", icon: <Info size={14} />, onClick: onInspect },
              { label: "Copy ID", icon: <Copy size={14} />, onClick: onCopyId },
              { label: "Kill", icon: <Skull size={14} />, onClick: () => onAction("kill"), disabled: !!busy || !isRunning, danger: true },
              { label: "Remove…", icon: <Trash2 size={14} />, onClick: () => onAction("remove"), disabled: !!busy, danger: true },
            ]}
          />
        </div>
      </td>
    </tr>
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

import clsx from "clsx";
import { Boxes, Container, Database, Download, Eraser, FileText, HardDrive, Info, Layers, Loader2, Network, Plus, RotateCw, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { TerminalView } from "../components/TerminalView";
import { Badge, Button, Checkbox, Empty, Field, IconButton, Input, Modal, Select } from "../components/ui";
import { api, formatBytes } from "../lib/ipc";
import type { NetworkInfo, PruneResult, PruneTarget, UsageRow, VolumeInfo } from "../lib/types";
import { useApp } from "../store";

export function JsonModal({ title, json, onClose }: { title: string; json: unknown; onClose: () => void }) {
  return (
    <Modal title={title} onClose={onClose} width="max-w-4xl">
      <pre className="whitespace-pre-wrap break-all rounded-lg bg-surface-1 p-4 font-mono text-xs leading-relaxed">{JSON.stringify(json, null, 2)}</pre>
    </Modal>
  );
}

function size(n?: number | null, text?: string | null) {
  if (n != null) return formatBytes(n);
  return text || "—";
}

// ---------------- System ----------------

const PRUNES: { target: PruneTarget; title: string; desc: string; allLabel?: string; icon: React.ReactNode }[] = [
  { target: "containers", title: "Stopped containers", desc: "Remove all containers that are not running.", icon: <Container size={16} /> },
  { target: "images", title: "Images", desc: "Remove dangling (untagged) images.", allLabel: "All unused images, not just dangling", icon: <Layers size={16} /> },
  { target: "volumes", title: "Volumes", desc: "Remove anonymous volumes not used by any container.", allLabel: "Named volumes too (data loss!)", icon: <Database size={16} /> },
  { target: "networks", title: "Networks", desc: "Remove networks not used by any container.", icon: <Network size={16} /> },
  { target: "build_cache", title: "Build cache", desc: "Remove dangling build cache.", allLabel: "All build cache", icon: <HardDrive size={16} /> },
];

export function SystemPanel({ endpoint, active }: { endpoint: string; active: boolean }) {
  const { error, toast } = useApp();
  const [rows, setRows] = useState<UsageRow[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [confirm, setConfirm] = useState<{ target: PruneTarget; title: string; allLabel?: string } | null>(null);
  const [all, setAll] = useState(false);
  const [withVolumes, setWithVolumes] = useState(false);
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<{ title: string; r: PruneResult } | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setRows(await api.dockerDiskUsage(endpoint));
    } catch (e) {
      error(e);
    } finally {
      setLoading(false);
    }
  }, [endpoint, error]);

  useEffect(() => {
    if (active) load();
  }, [active, load]);

  const run = async () => {
    if (!confirm) return;
    setRunning(true);
    try {
      const r = await api.dockerPrune(endpoint, confirm.target, { all, volumes: withVolumes });
      setResult({ title: confirm.title, r });
      toast(`${confirm.title}: removed ${r.removed}${r.reclaimed != null ? `, freed ${formatBytes(r.reclaimed)}` : r.reclaimedText ? `, freed ${r.reclaimedText}` : ""}`, "success");
      setConfirm(null);
      load();
    } catch (e) {
      error(e);
    } finally {
      setRunning(false);
    }
  };

  const totalReclaim = rows?.reduce((a, r) => a + (r.reclaimable ?? 0), 0) ?? 0;

  return (
    <div className="space-y-8 p-5">
      <section>
        <div className="mb-3 flex items-center justify-between">
          <h3 className="text-xs font-semibold uppercase tracking-wider text-fg-subtle">Disk usage</h3>
          <IconButton title="Refresh" onClick={load}>
            <RotateCw size={14} className={loading ? "animate-spin" : ""} />
          </IconButton>
        </div>
        {!rows ? (
          <div className="flex items-center gap-2 text-sm text-fg-muted">
            <Loader2 size={14} className="animate-spin" /> Measuring…
          </div>
        ) : (
          <div className="grid grid-cols-2 gap-3 xl:grid-cols-4">
            {rows.map((r) => {
              const pct = r.size && r.reclaimable != null && r.size > 0 ? Math.min(100, (r.reclaimable / r.size) * 100) : null;
              return (
                <div key={r.kind} className="rounded-xl border border-border bg-surface-2 p-4">
                  <div className="text-xs text-fg-subtle">{r.kind}</div>
                  <div className="mt-1 text-xl font-semibold">{size(r.size, r.sizeText)}</div>
                  <div className="mt-1 text-xs text-fg-muted">
                    {r.total} total · {r.active} in use
                  </div>
                  <div className="mt-3 h-1.5 overflow-hidden rounded bg-surface-4">
                    <div className="h-full bg-warn/70" style={{ width: `${pct ?? 0}%` }} />
                  </div>
                  <div className="mt-1.5 text-xs text-warn/90">{size(r.reclaimable, r.reclaimableText)} reclaimable</div>
                </div>
              );
            })}
          </div>
        )}
        {rows && totalReclaim > 0 && <div className="mt-2 text-xs text-fg-subtle">About {formatBytes(totalReclaim)} could be reclaimed in total.</div>}
      </section>

      <section>
        <h3 className="mb-3 text-xs font-semibold uppercase tracking-wider text-fg-subtle">Clean up</h3>
        <div className="divide-y divide-border overflow-hidden rounded-xl border border-border">
          {PRUNES.map((p) => (
            <div key={p.target} className="flex items-center gap-4 bg-surface-1 px-4 py-3">
              <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-surface-3 text-fg-muted">{p.icon}</div>
              <div className="min-w-0 flex-1">
                <div className="text-sm font-medium">{p.title}</div>
                <div className="text-xs text-fg-subtle">{p.desc}</div>
              </div>
              <Button
                onClick={() => {
                  setAll(false);
                  setWithVolumes(false);
                  setConfirm({ target: p.target, title: `Prune ${p.title.toLowerCase()}`, allLabel: p.allLabel });
                }}
              >
                <Eraser size={14} /> Prune
              </Button>
            </div>
          ))}
          <div className="flex items-center gap-4 bg-danger/5 px-4 py-3">
            <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-danger/15 text-danger">
              <Boxes size={16} />
            </div>
            <div className="min-w-0 flex-1">
              <div className="text-sm font-medium">System prune</div>
              <div className="text-xs text-fg-subtle">Stopped containers, unused networks, dangling images and build cache in one go.</div>
            </div>
            <Button
              variant="danger"
              onClick={() => {
                setAll(false);
                setWithVolumes(false);
                setConfirm({ target: "system", title: "System prune", allLabel: "All unused images, not just dangling" });
              }}
            >
              <Eraser size={14} /> System prune
            </Button>
          </div>
        </div>
      </section>

      {confirm && (
        <Modal
          title={confirm.title}
          onClose={() => !running && setConfirm(null)}
          width="max-w-md"
          footer={
            <>
              <Button variant="ghost" onClick={() => setConfirm(null)} disabled={running}>
                Cancel
              </Button>
              <Button variant="danger" onClick={run} disabled={running}>
                {running ? <Loader2 size={14} className="animate-spin" /> : <Eraser size={14} />} {running ? "Pruning…" : "Prune"}
              </Button>
            </>
          }
        >
          <div className="space-y-3 text-sm">
            <p>This permanently removes the selected Docker objects on this endpoint. It cannot be undone.</p>
            {confirm.allLabel && <Checkbox label={confirm.allLabel} checked={all} onChange={setAll} />}
            {confirm.target === "system" && <Checkbox label="Also remove unused volumes (data loss!)" checked={withVolumes} onChange={setWithVolumes} />}
          </div>
        </Modal>
      )}
      {result && (
        <Modal title={`${result.title}: done`} onClose={() => setResult(null)} width="max-w-lg">
          <div className="mb-3 text-sm">
            Removed <b>{result.r.removed}</b> object{result.r.removed === 1 ? "" : "s"}
            {result.r.reclaimed != null && (
              <>
                , freed <b>{formatBytes(result.r.reclaimed)}</b>
              </>
            )}
            {result.r.reclaimedText && (
              <>
                , freed <b>{result.r.reclaimedText}</b>
              </>
            )}
            .
          </div>
          {result.r.details.length > 0 && (
            <pre className="max-h-72 overflow-auto rounded-lg bg-surface-1 p-3 font-mono text-xs leading-relaxed text-fg-muted">{result.r.details.join("\n")}</pre>
          )}
        </Modal>
      )}
    </div>
  );
}

// ---------------- Volumes ----------------

export function VolumesPanel({ endpoint, active, filter }: { endpoint: string; active: boolean; filter: string }) {
  const { error, toast } = useApp();
  const [list, setList] = useState<VolumeInfo[] | null>(null);
  const [inspect, setInspect] = useState<{ name: string; json: unknown } | null>(null);
  const [confirm, setConfirm] = useState<VolumeInfo | null>(null);
  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState("");
  const [showAnon, setShowAnon] = useState(false);

  const load = useCallback(async () => {
    try {
      setList(await api.dockerVolumes(endpoint));
    } catch (e) {
      error(e);
    }
  }, [endpoint, error]);
  useEffect(() => {
    if (active) load();
  }, [active, load]);

  const q = filter.toLowerCase();
  const rows = (list ?? []).filter((v) => (showAnon || !v.anonymous) && (!q || [v.name, v.project ?? "", ...v.usedBy].some((s) => s.toLowerCase().includes(q))));
  const anonCount = (list ?? []).filter((v) => v.anonymous).length;

  return (
    <>
      <div className="flex items-center gap-4 border-b border-border px-5 py-2">
        <span className="text-xs text-fg-subtle">
          {list ? `${list.length} volumes · ${list.filter((v) => v.usedBy.length === 0).length} unused` : "Loading…"}
        </span>
        <div className="flex-1" />
        <Checkbox label={`Show anonymous (${anonCount})`} checked={showAnon} onChange={setShowAnon} />
        <Button size="sm" onClick={() => setCreating(true)}>
          <Plus size={13} /> New volume
        </Button>
      </div>
      {list && rows.length === 0 ? (
        <Empty icon={<Database size={32} />} title="No volumes" />
      ) : (
        <table className="w-full table-fixed text-sm">
          <colgroup>
            <col />
            <col className="w-40" />
            <col className="w-[30%]" />
            <col className="w-24" />
          </colgroup>
          <thead className="sticky top-0 z-10 bg-surface-0 text-left text-[11px] uppercase tracking-wider text-fg-subtle shadow-[0_1px_0_var(--color-border)]">
            <tr>
              <th className="py-2.5 pl-5 font-medium">Volume</th>
              <th className="px-3 font-medium">Project</th>
              <th className="px-3 font-medium">Used by</th>
              <th className="pr-5 text-right font-medium">Actions</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((v) => (
              <tr key={v.name} className="border-b border-border/50 hover:bg-surface-2">
                <td className="py-2.5 pl-5 pr-3">
                  <div className="flex items-center gap-3">
                    <Database size={14} className={v.usedBy.length ? "text-sky-400" : "text-fg-subtle"} />
                    <div className="min-w-0">
                      <div className="truncate font-medium" title={v.name}>
                        {v.anonymous ? `${v.name.slice(0, 12)}…` : v.name}
                        {v.anonymous && <span className="ml-2 text-xs font-normal text-fg-subtle">anonymous</span>}
                      </div>
                      <div className="truncate font-mono text-[11px] text-fg-subtle" title={v.mountpoint}>
                        {v.driver} · {v.mountpoint}
                      </div>
                    </div>
                  </div>
                </td>
                <td className="truncate px-3 text-fg-muted">{v.project ?? "—"}</td>
                <td className="px-3">
                  {v.usedBy.length ? (
                    <div className="truncate text-fg-muted" title={v.usedBy.join(", ")}>
                      {v.usedBy.join(", ")}
                    </div>
                  ) : (
                    <Badge tone="warn">unused</Badge>
                  )}
                </td>
                <td className="pr-5">
                  <div className="flex justify-end gap-0.5">
                    <IconButton
                      title="Inspect"
                      onClick={async () => {
                        try {
                          setInspect({ name: v.name, json: await api.dockerVolumeInspect(endpoint, v.name) });
                        } catch (e) {
                          error(e);
                        }
                      }}
                    >
                      <Info size={14} />
                    </IconButton>
                    <IconButton title="Remove" onClick={() => setConfirm(v)}>
                      <Trash2 size={14} className="text-danger" />
                    </IconButton>
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {inspect && <JsonModal title={`Volume · ${inspect.name}`} json={inspect.json} onClose={() => setInspect(null)} />}
      {confirm && (
        <Modal
          title="Remove volume"
          onClose={() => setConfirm(null)}
          width="max-w-md"
          footer={
            <>
              <Button variant="ghost" onClick={() => setConfirm(null)}>
                Cancel
              </Button>
              <Button
                variant="danger"
                onClick={async () => {
                  try {
                    await api.dockerVolumeRemove(endpoint, confirm.name, false);
                    toast(`Removed volume ${confirm.name}`, "success");
                    setConfirm(null);
                    load();
                  } catch (e) {
                    error(e);
                  }
                }}
              >
                <Trash2 size={14} /> Remove
              </Button>
            </>
          }
        >
          <p className="text-sm leading-relaxed">
            Delete volume <b className="break-all">{confirm.name}</b> and all of its data?
            {confirm.usedBy.length > 0 && <span className="text-warn"> It is used by {confirm.usedBy.join(", ")}; Docker will refuse while those containers exist.</span>}
          </p>
        </Modal>
      )}
      {creating && (
        <Modal
          title="New volume"
          onClose={() => setCreating(false)}
          width="max-w-sm"
          footer={
            <Button
              variant="primary"
              disabled={!newName}
              onClick={async () => {
                try {
                  await api.dockerVolumeCreate(endpoint, newName);
                  setCreating(false);
                  setNewName("");
                  load();
                } catch (e) {
                  error(e);
                }
              }}
            >
              Create
            </Button>
          }
        >
          <Input autoFocus placeholder="volume name" value={newName} onChange={(e) => setNewName(e.target.value)} />
        </Modal>
      )}
    </>
  );
}

// ---------------- Networks ----------------

export function NetworksPanel({ endpoint, active, filter }: { endpoint: string; active: boolean; filter: string }) {
  const { error, toast } = useApp();
  const [list, setList] = useState<NetworkInfo[] | null>(null);
  const [inspect, setInspect] = useState<{ name: string; json: unknown } | null>(null);
  const [confirm, setConfirm] = useState<NetworkInfo | null>(null);
  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState("");
  const [internal, setInternal] = useState(false);

  const load = useCallback(async () => {
    try {
      setList(await api.dockerNetworks(endpoint));
    } catch (e) {
      error(e);
    }
  }, [endpoint, error]);
  useEffect(() => {
    if (active) load();
  }, [active, load]);

  const q = filter.toLowerCase();
  const rows = (list ?? []).filter((n) => !q || [n.name, n.project ?? "", n.driver, ...n.containers].some((s) => s.toLowerCase().includes(q)));

  return (
    <>
      <div className="flex items-center gap-4 border-b border-border px-5 py-2">
        <span className="text-xs text-fg-subtle">{list ? `${list.length} networks` : "Loading…"}</span>
        <div className="flex-1" />
        <Button size="sm" onClick={() => setCreating(true)}>
          <Plus size={13} /> New network
        </Button>
      </div>
      <table className="w-full table-fixed text-sm">
        <colgroup>
          <col />
          <col className="w-28" />
          <col className="w-44" />
          <col className="w-[30%]" />
          <col className="w-24" />
        </colgroup>
        <thead className="sticky top-0 z-10 bg-surface-0 text-left text-[11px] uppercase tracking-wider text-fg-subtle shadow-[0_1px_0_var(--color-border)]">
          <tr>
            <th className="py-2.5 pl-5 font-medium">Network</th>
            <th className="px-3 font-medium">Driver</th>
            <th className="px-3 font-medium">Subnet</th>
            <th className="px-3 font-medium">Containers</th>
            <th className="pr-5 text-right font-medium">Actions</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((n) => (
            <tr key={n.id} className="border-b border-border/50 hover:bg-surface-2">
              <td className="py-2.5 pl-5 pr-3">
                <div className="flex items-center gap-3">
                  <Network size={14} className={n.containers.length ? "text-sky-400" : "text-fg-subtle"} />
                  <div className="min-w-0">
                    <div className="truncate font-medium">
                      {n.name}
                      {n.builtin && <span className="ml-2 text-xs font-normal text-fg-subtle">built-in</span>}
                      {n.internal && <span className="ml-2 text-xs font-normal text-warn">internal</span>}
                    </div>
                    <div className="truncate font-mono text-[11px] text-fg-subtle">
                      {n.id.slice(0, 12)}
                      {n.project ? ` · ${n.project}` : ""}
                    </div>
                  </div>
                </div>
              </td>
              <td className="px-3 text-fg-muted">
                {n.driver}
                <span className="text-fg-subtle"> · {n.scope}</span>
              </td>
              <td className="px-3 font-mono text-xs text-fg-muted">
                {n.subnet ?? "—"}
                {n.gateway && <div className="text-[11px] text-fg-subtle">gw {n.gateway}</div>}
              </td>
              <td className="px-3">
                {n.containers.length ? (
                  <div className="truncate text-fg-muted" title={n.containers.join(", ")}>
                    {n.containers.join(", ")}
                  </div>
                ) : (
                  <span className="text-fg-subtle">—</span>
                )}
              </td>
              <td className="pr-5">
                <div className="flex justify-end gap-0.5">
                  <IconButton
                    title="Inspect"
                    onClick={async () => {
                      try {
                        setInspect({ name: n.name, json: await api.dockerNetworkInspect(endpoint, n.id) });
                      } catch (e) {
                        error(e);
                      }
                    }}
                  >
                    <Info size={14} />
                  </IconButton>
                  <IconButton title={n.builtin ? "Built-in networks can't be removed" : "Remove"} disabled={n.builtin} onClick={() => setConfirm(n)}>
                    <Trash2 size={14} className={n.builtin ? "" : "text-danger"} />
                  </IconButton>
                </div>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {inspect && <JsonModal title={`Network · ${inspect.name}`} json={inspect.json} onClose={() => setInspect(null)} />}
      {confirm && (
        <Modal
          title="Remove network"
          onClose={() => setConfirm(null)}
          width="max-w-md"
          footer={
            <>
              <Button variant="ghost" onClick={() => setConfirm(null)}>
                Cancel
              </Button>
              <Button
                variant="danger"
                onClick={async () => {
                  try {
                    await api.dockerNetworkRemove(endpoint, confirm.id);
                    toast(`Removed network ${confirm.name}`, "success");
                    setConfirm(null);
                    load();
                  } catch (e) {
                    error(e);
                  }
                }}
              >
                <Trash2 size={14} /> Remove
              </Button>
            </>
          }
        >
          <p className="text-sm leading-relaxed">
            Remove network <b>{confirm.name}</b>?
            {confirm.containers.length > 0 && <span className="text-warn"> Containers still attached: {confirm.containers.join(", ")}.</span>}
          </p>
        </Modal>
      )}
      {creating && (
        <Modal
          title="New bridge network"
          onClose={() => setCreating(false)}
          width="max-w-sm"
          footer={
            <Button
              variant="primary"
              disabled={!newName}
              onClick={async () => {
                try {
                  await api.dockerNetworkCreate(endpoint, newName, internal);
                  setCreating(false);
                  setNewName("");
                  load();
                } catch (e) {
                  error(e);
                }
              }}
            >
              Create
            </Button>
          }
        >
          <div className="space-y-3">
            <Input autoFocus placeholder="network name" value={newName} onChange={(e) => setNewName(e.target.value)} />
            <Checkbox label="Internal (no outbound access)" checked={internal} onChange={setInternal} />
          </div>
        </Modal>
      )}
    </>
  );
}

// ---------------- Image pull ----------------

export function PullDialog({ endpoint, onClose, onDone }: { endpoint: string; onClose: () => void; onDone: () => void }) {
  const { secrets, vault, setUnlockPrompt } = useApp();
  const [ref, setRef] = useState("");
  const [secretId, setSecretId] = useState("");
  const [started, setStarted] = useState<string | null>(null);
  const [finished, setFinished] = useState(false);
  const creds = secrets.filter((s) => (s.kind === "password" || s.kind === "token") && s.username);
  const start = () => ref.trim() && setStarted(ref.trim());
  return (
    <Modal
      title="Pull image"
      onClose={onClose}
      width="max-w-3xl"
      footer={
        started ? (
          <Button variant="primary" onClick={onClose}>
            {finished ? "Close" : "Run in background"}
          </Button>
        ) : (
          <Button variant="primary" disabled={!ref.trim()} onClick={start}>
            <Download size={14} /> Pull
          </Button>
        )
      }
    >
      {!started ? (
        <form
          className="space-y-4"
          onSubmit={(e) => {
            e.preventDefault();
            start();
          }}
        >
          <Field label="Image reference">
            <Input autoFocus placeholder="nginx:alpine · ghcr.io/org/app:1.2 · registry.example.com:5000/app" value={ref} onChange={(e) => setRef(e.target.value)} />
          </Field>
          <Field
            label="Registry credentials"
            hint={
              vault.unlocked
                ? "A vault password or token secret with a username. Leave empty for public images."
                : undefined
            }
          >
            {vault.unlocked ? (
              <Select value={secretId} onChange={(e) => setSecretId(e.target.value)}>
                <option value="">— none (public image) —</option>
                {creds.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name} ({s.username})
                  </option>
                ))}
              </Select>
            ) : (
              <div className="flex items-center gap-3 text-sm text-fg-muted">
                Vault is locked.
                <Button size="sm" type="button" onClick={() => setUnlockPrompt(true)}>
                  Unlock to use credentials
                </Button>
              </div>
            )}
          </Field>
          <button type="submit" className="hidden" />
        </form>
      ) : (
        <div className="h-72 overflow-hidden rounded-lg border border-border">
          <TerminalView
            active
            readOnly
            open={(_c, _r, ch) => api.dockerImagePull(endpoint, started, secretId || null, ch)}
            onClosed={() => {
              setFinished(true);
              onDone();
            }}
          />
        </div>
      )}
    </Modal>
  );
}

// ---------------- Combined logs ----------------

export function MultiLogsView({ endpoint, containers, active }: { endpoint: string; containers: { id: string; name: string }[]; active: boolean }) {
  const settings = useApp((s) => s.settings);
  const [tail, setTail] = useState(Math.min(settings?.dockerLogTail ?? 200, 200));
  const [timestamps, setTimestamps] = useState(false);
  const [follow, setFollow] = useState(true);
  const [gen, setGen] = useState(0);
  return (
    <TerminalView
      active={active}
      readOnly
      generation={gen}
      open={(_c, _r, ch) => api.dockerLogsMulti(endpoint, containers, tail, timestamps, follow, ch)}
      toolbar={
        <div className="flex items-center gap-4 border-b border-border px-4 py-2 text-sm">
          <FileText size={15} className="text-fg-subtle" />
          <span className="truncate text-fg-muted" title={containers.map((c) => c.name).join(", ")}>
            {containers.length} container{containers.length === 1 ? "" : "s"}
          </span>
          <label className="flex items-center gap-2 text-fg-muted">
            Tail
            <Input type="number" className="h-7 w-20" value={tail} onChange={(e) => setTail(Number(e.target.value) || 0)} />
          </label>
          <Checkbox label="Follow" checked={follow} onChange={setFollow} />
          <Checkbox label="Timestamps" checked={timestamps} onChange={setTimestamps} />
          <Button size="sm" onClick={() => setGen((g) => g + 1)}>
            <RotateCw size={13} /> Apply
          </Button>
          <div className="flex-1" />
          <span className={clsx("text-xs text-fg-subtle")}>Ctrl+Shift+F to search</span>
        </div>
      }
    />
  );
}

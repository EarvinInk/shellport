import { ask } from "@tauri-apps/plugin-dialog";
import { Channel } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import clsx from "clsx";
import { ArrowDownToLine, ArrowLeft, ArrowUpFromLine, File, Folder, FolderPlus, Home, Link2, Pencil, RefreshCw, Shield, Trash2, X } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { Button, IconButton, Input, Modal } from "../components/ui";
import { api, errMsg, formatBytes, modeString } from "../lib/ipc";
import type { FileEntry, TransferEvent } from "../lib/types";
import { useApp } from "../store";

interface Transfer {
  id: number;
  dir: "up" | "down";
  name: string;
  bytes: number;
  total: number;
  files: number;
  state: "running" | "done" | "error" | "cancelled";
  message?: string;
  taskId: string;
}

let tSeq = 1;

function parent(p: string) {
  if (p === "/" || p === "") return "/";
  const t = p.replace(/\/+$/, "");
  const i = t.lastIndexOf("/");
  return i <= 0 ? "/" : t.slice(0, i);
}
function join(d: string, n: string) {
  return d.endsWith("/") ? d + n : `${d}/${n}`;
}

function usePane(loader: (p: string) => Promise<FileEntry[]>, initial: () => Promise<string>) {
  const [path, setPath] = useState("");
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [history, setHistory] = useState<string[]>([]);
  const load = useCallback(
    async (p: string, push = true) => {
      setLoading(true);
      setErr(null);
      try {
        const e = await loader(p);
        setEntries(e);
        setPath((old) => {
          if (push && old && old !== p) setHistory((h) => [...h.slice(-30), old]);
          return p;
        });
        setSelected(null);
      } catch (e) {
        setErr(errMsg(e));
      } finally {
        setLoading(false);
      }
    },
    [loader],
  );
  useEffect(() => {
    initial()
      .then((p) => load(p, false))
      .catch((e) => setErr(errMsg(e)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const back = () => {
    const prev = history[history.length - 1];
    if (prev) {
      setHistory((h) => h.slice(0, -1));
      load(prev, false);
    }
  };
  return { path, entries, loading, err, selected, setSelected, load, back, refresh: () => load(path, false) };
}

/** Compact modified date, e.g. "10 Nov 2026, 10:35" (full time in the tooltip). */
const SHORT_DATE = new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" });

/** Must match `sftp::EXISTS_PREFIX` in the backend. */
const EXISTS_PREFIX = "local files exist:";

export function SftpView({ hostId, active }: { hostId: string; active: boolean }) {
  const { error, toast } = useApp();
  const localLoader = useCallback((p: string) => api.localList(p), []);
  const remoteLoader = useCallback((p: string) => api.sftpList(hostId, p), [hostId]);
  const local = usePane(localLoader, api.localHome);
  const remote = usePane(remoteLoader, () => api.sftpHome(hostId));
  const [transfers, setTransfers] = useState<Transfer[]>([]);
  const [dialog, setDialog] = useState<null | { kind: "mkdir" | "rename" | "delete" | "chmod"; entry?: FileEntry }>(null);
  const remoteRef = useRef(remote);
  remoteRef.current = remote;
  const localRef = useRef(local);
  localRef.current = local;

  const run = (dir: "up" | "down", source: string, destDir: string, overwrite = false) => {
    const id = tSeq++;
    const name = source.split("/").filter(Boolean).pop() ?? source;
    const taskId = `xfer:${hostId}:${id}:${Date.now()}`;
    setTransfers((t) => [{ id, dir, name, bytes: 0, total: 0, files: 0, state: "running", taskId }, ...t]);
    const upd = (p: Partial<Transfer>) => setTransfers((t) => t.map((x) => (x.id === id ? { ...x, ...p } : x)));
    const ch = new Channel<TransferEvent>();
    ch.onmessage = (ev) => {
      if (ev.type === "progress") upd({ bytes: ev.bytes, total: ev.total, files: ev.filesDone });
      if (ev.type === "done") {
        upd({ state: "done", bytes: ev.bytes, total: ev.bytes, files: ev.files });
        if (dir === "up") remoteRef.current.refresh();
        else localRef.current.refresh();
      }
      if (ev.type === "error") {
        if (ev.message.startsWith(EXISTS_PREFIX)) {
          // Nothing was written yet: drop the row and ask before overwriting.
          setTransfers((t) => t.filter((x) => x.id !== id));
          const n = Number(ev.message.slice(EXISTS_PREFIX.length)) || 1;
          ask(`${n} file${n === 1 ? "" : "s"} in ${destDir} would be overwritten by "${name}". Overwrite?`, {
            title: "Overwrite local files?",
            kind: "warning",
            okLabel: "Overwrite",
            cancelLabel: "Cancel",
          }).then((ok) => ok && run(dir, source, destDir, true));
        } else upd(ev.message === "cancelled" ? { state: "cancelled" } : { state: "error", message: ev.message });
      }
    };
    const p = dir === "up" ? api.sftpUpload(hostId, source, destDir, taskId, ch) : api.sftpDownload(hostId, source, destDir, taskId, ch, overwrite);
    p.catch((e) => {
      const m = errMsg(e);
      if (!m.startsWith(EXISTS_PREFIX)) upd({ state: "error", message: m });
    });
  };

  // Files dropped from the desktop file manager → upload to remote cwd.
  useEffect(() => {
    if (!active) return;
    const un = getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type === "drop" && remoteRef.current.path) {
        for (const p of e.payload.paths) run("up", p, remoteRef.current.path);
      }
    });
    return () => {
      un.then((f) => f());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, hostId]);

  const remoteSel = remote.entries.find((e) => e.path === remote.selected);
  const localSel = local.entries.find((e) => e.path === local.selected);

  const doDialog = async (value: string) => {
    if (!dialog) return;
    try {
      if (dialog.kind === "mkdir") await api.sftpMkdir(hostId, join(remote.path, value));
      if (dialog.kind === "rename" && dialog.entry) await api.sftpRename(hostId, dialog.entry.path, join(parent(dialog.entry.path), value));
      if (dialog.kind === "delete" && dialog.entry) await api.sftpRemove(hostId, dialog.entry.path);
      if (dialog.kind === "chmod" && dialog.entry) {
        const mode = parseInt(value, 8);
        if (Number.isNaN(mode)) throw new Error("Mode must be octal, e.g. 644");
        await api.sftpChmod(hostId, dialog.entry.path, mode);
      }
      setDialog(null);
      remote.refresh();
    } catch (e) {
      error(e);
    }
  };

  return (
    <div className="flex h-full flex-col">
      <div className="grid min-h-0 flex-1 grid-cols-2 gap-px bg-border">
        <Pane
          title="Local"
          pane={local}
          onOpen={(e) => e.isDir && local.load(e.path)}
          dragKind="local"
          onDropFrom={(kind, path) => kind === "remote" && run("down", path, local.path)}
          actions={
            <Button size="sm" variant="primary" disabled={!localSel || !remote.path} onClick={() => localSel && run("up", localSel.path, remote.path)}>
              <ArrowUpFromLine size={13} /> Upload
            </Button>
          }
        />
        <Pane
          title="Remote"
          pane={remote}
          onOpen={(e) => e.isDir && remote.load(e.path)}
          dragKind="remote"
          onDropFrom={(kind, path) => kind === "local" && run("up", path, remote.path)}
          actions={
            <>
              <Button size="sm" variant="primary" disabled={!remoteSel || !local.path} onClick={() => remoteSel && run("down", remoteSel.path, local.path)}>
                <ArrowDownToLine size={13} /> Download
              </Button>
              <IconButton title="New folder" onClick={() => setDialog({ kind: "mkdir" })}>
                <FolderPlus size={14} />
              </IconButton>
              <IconButton title="Rename" disabled={!remoteSel} onClick={() => setDialog({ kind: "rename", entry: remoteSel })}>
                <Pencil size={14} />
              </IconButton>
              <IconButton title="Permissions" disabled={!remoteSel} onClick={() => setDialog({ kind: "chmod", entry: remoteSel })}>
                <Shield size={14} />
              </IconButton>
              <IconButton title="Delete" disabled={!remoteSel} onClick={() => setDialog({ kind: "delete", entry: remoteSel })}>
                <Trash2 size={14} />
              </IconButton>
            </>
          }
        />
      </div>
      {transfers.length > 0 && (
        <div className="max-h-40 overflow-y-auto border-t border-border bg-surface-1">
          <div className="flex items-center justify-between px-3 py-1 text-xs text-fg-subtle">
            <span>Transfers</span>
            <button className="hover:text-fg cursor-pointer" onClick={() => setTransfers((t) => t.filter((x) => x.state === "running"))}>
              Clear finished
            </button>
          </div>
          {transfers.map((t) => (
            <div key={t.id} className="flex items-center gap-3 px-3 py-1 text-xs">
              {t.dir === "up" ? <ArrowUpFromLine size={12} /> : <ArrowDownToLine size={12} />}
              <span className="w-48 truncate">{t.name}</span>
              <div className="h-1.5 flex-1 overflow-hidden rounded bg-surface-3">
                <div
                  className={clsx("h-full", t.state === "error" || t.state === "cancelled" ? "bg-danger" : t.state === "done" ? "bg-ok" : "bg-accent")}
                  style={{ width: `${t.total ? Math.min(100, (t.bytes / t.total) * 100) : t.state === "done" ? 100 : 5}%` }}
                />
              </div>
              <span className="w-44 text-right text-fg-muted">
                {t.state === "error" ? (
                  <span className="text-danger" title={t.message}>
                    failed: {t.message}
                  </span>
                ) : t.state === "cancelled" ? (
                  <span className="text-warn">cancelled</span>
                ) : (
                  `${formatBytes(t.bytes)} / ${formatBytes(t.total)} · ${t.files} files`
                )}
              </span>
              <span className="w-6">
                {t.state === "running" && (
                  <IconButton title="Cancel transfer" className="h-6 w-6" onClick={() => api.taskCancel(t.taskId)}>
                    <X size={12} />
                  </IconButton>
                )}
              </span>
            </div>
          ))}
        </div>
      )}
      {dialog && (
        <PromptDialog
          kind={dialog.kind}
          entry={dialog.entry}
          onClose={() => setDialog(null)}
          onSubmit={(v) => {
            doDialog(v);
            if (dialog.kind === "delete") toast(`Deleting ${dialog.entry?.name}…`);
          }}
        />
      )}
    </div>
  );
}

type PaneState = ReturnType<typeof usePane>;

function Pane({
  title,
  pane,
  onOpen,
  actions,
  dragKind,
  onDropFrom,
}: {
  title: string;
  pane: PaneState;
  onOpen: (e: FileEntry) => void;
  actions: React.ReactNode;
  dragKind: "local" | "remote";
  onDropFrom: (kind: string, path: string) => void;
}) {
  const [editPath, setEditPath] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [showHidden, setShowHidden] = useState(false);
  const [dragOver, setDragOver] = useState(false);
  const entries = pane.entries.filter((e) => (showHidden || !e.name.startsWith(".")) && (!filter || e.name.toLowerCase().includes(filter.toLowerCase())));
  return (
    <div
      className={clsx("flex min-h-0 flex-col bg-surface-0", dragOver && "ring-2 ring-inset ring-accent")}
      onDragOver={(e) => {
        if (e.dataTransfer.types.includes("application/x-shellport")) {
          e.preventDefault();
          setDragOver(true);
        }
      }}
      onDragLeave={() => setDragOver(false)}
      onDrop={(e) => {
        setDragOver(false);
        const raw = e.dataTransfer.getData("application/x-shellport");
        if (raw) {
          const { kind, path } = JSON.parse(raw);
          onDropFrom(kind, path);
        }
      }}
    >
      <div className="flex items-center gap-1 border-b border-border px-2 py-1.5">
        <span className="w-14 text-xs font-semibold uppercase tracking-wider text-fg-subtle">{title}</span>
        <IconButton title="Back" onClick={pane.back}>
          <ArrowLeft size={14} />
        </IconButton>
        <IconButton title="Up" onClick={() => pane.load(parent(pane.path))}>
          <span className="text-xs">..</span>
        </IconButton>
        <IconButton title="Home" onClick={() => pane.load(title === "Local" ? "~" : ".")}>
          <Home size={14} />
        </IconButton>
        {editPath !== null ? (
          <form
            className="flex-1"
            onSubmit={(e) => {
              e.preventDefault();
              pane.load(editPath);
              setEditPath(null);
            }}
          >
            <Input autoFocus value={editPath} onChange={(e) => setEditPath(e.target.value)} onBlur={() => setEditPath(null)} className="h-7 font-mono text-xs" />
          </form>
        ) : (
          <button className="flex-1 truncate rounded px-2 py-1 text-left font-mono text-xs text-fg-muted hover:bg-surface-3 cursor-text" onClick={() => setEditPath(pane.path)}>
            {pane.path}
          </button>
        )}
        <IconButton title="Refresh" onClick={pane.refresh}>
          <RefreshCw size={14} className={pane.loading ? "animate-spin" : ""} />
        </IconButton>
      </div>
      <div className="flex items-center gap-1 border-b border-border px-2 py-1">
        <Input value={filter} onChange={(e) => setFilter(e.target.value)} placeholder="Filter" className="h-7 w-40 text-xs" />
        <label className="flex items-center gap-1 px-2 text-xs text-fg-muted">
          <input type="checkbox" checked={showHidden} onChange={(e) => setShowHidden(e.target.checked)} /> hidden
        </label>
        <div className="flex-1" />
        {actions}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {pane.err && <div className="m-2 rounded bg-danger/10 p-2 text-sm text-danger">{pane.err}</div>}
        <table className="w-full table-fixed text-sm">
          <thead className="sticky top-0 bg-surface-0 text-left text-[11px] uppercase text-fg-subtle">
            <tr>
              <th className="px-2 py-1 font-medium">Name</th>
              <th className="w-20 px-2 font-medium">Size</th>
              <th className="w-24 px-2 font-medium">Mode</th>
              <th className="w-36 px-2 font-medium">Modified</th>
            </tr>
          </thead>
          <tbody>
            {entries.map((e) => (
              <tr
                key={e.path}
                draggable
                onDragStart={(ev) => ev.dataTransfer.setData("application/x-shellport", JSON.stringify({ kind: dragKind, path: e.path }))}
                onClick={() => pane.setSelected(e.path)}
                onDoubleClick={() => onOpen(e)}
                className={clsx("cursor-default select-none", pane.selected === e.path ? "bg-accent/20" : "hover:bg-surface-2")}
              >
                <td className="truncate px-2 py-1">
                  <span className="inline-flex items-center gap-2">
                    {e.isDir ? <Folder size={14} className="text-sky-400" /> : <File size={14} className="text-fg-subtle" />}
                    {e.name}
                    {e.isSymlink && <Link2 size={11} className="text-fg-subtle" />}
                  </span>
                </td>
                <td className="px-2 text-xs text-fg-muted">{e.isDir ? "" : formatBytes(e.size)}</td>
                <td className="px-2 font-mono text-xs text-fg-subtle">{modeString(e.permissions, e.isDir)}</td>
                <td className="whitespace-nowrap px-2 text-xs text-fg-subtle" title={e.mtime ? new Date(e.mtime * 1000).toLocaleString() : undefined}>
                  {e.mtime ? SHORT_DATE.format(new Date(e.mtime * 1000)) : ""}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function PromptDialog({ kind, entry, onClose, onSubmit }: { kind: "mkdir" | "rename" | "delete" | "chmod"; entry?: FileEntry; onClose: () => void; onSubmit: (v: string) => void }) {
  const [v, setV] = useState(kind === "rename" ? entry?.name ?? "" : kind === "chmod" ? ((entry?.permissions ?? 0o644) & 0o7777).toString(8) : "");
  const title = { mkdir: "New folder", rename: "Rename", delete: "Delete", chmod: "Change permissions" }[kind];
  return (
    <Modal
      title={title}
      onClose={onClose}
      width="max-w-sm"
      footer={
        <Button variant={kind === "delete" ? "danger" : "primary"} onClick={() => onSubmit(v)} disabled={kind !== "delete" && !v}>
          {kind === "delete" ? "Delete" : "OK"}
        </Button>
      }
    >
      {kind === "delete" ? (
        <p className="text-sm">
          Permanently delete <b>{entry?.path}</b>
          {entry?.isDir ? " and everything inside it" : ""}? This cannot be undone.
        </p>
      ) : (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            onSubmit(v);
          }}
        >
          <Input autoFocus value={v} onChange={(e) => setV(e.target.value)} placeholder={kind === "chmod" ? "755" : "name"} />
        </form>
      )}
    </Modal>
  );
}

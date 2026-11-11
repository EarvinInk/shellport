import { ask } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import clsx from "clsx";
import { listen } from "@tauri-apps/api/event";
import { Archive, ArrowDownToLine, ArrowLeft, ArrowLeftRight, ArrowUpFromLine, ExternalLink, File, FilePen, Folder, FolderPlus, Home, Link2, PackageOpen, Pencil, RefreshCw, Shield, ShieldAlert, Star, Trash2, X } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { TransferList } from "../components/TransfersPanel";
import { Button, Field, IconButton, Input, Modal, Select } from "../components/ui";
import { api, errMsg, formatBytes, modeString } from "../lib/ipc";
import type { Endpoint, FileEntry, LocalEditView, TransferView } from "../lib/types";
import { useApp } from "../store";

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

/** Must match `sudo::NEED_PASSWORD` and `remote_ops::DEST_EXISTS` in the backend. */
const NEED_PASSWORD = "sudo password needed:";
const DEST_EXISTS = "folder exists:";
const ARCHIVE = /\.(tar|tar\.gz|tgz|tar\.xz|txz|tar\.bz2|tbz2|tar\.zst|zip)$/i;

/** Does a transfer touch this host? */
const touches = (t: TransferView, hostId: string) => [t.source, t.destDir].some((e) => e.kind === "remote" && e.hostId === hostId);

export function SftpView({ hostId, active }: { hostId: string; active: boolean }) {
  const { error, toast } = useApp();
  const localLoader = useCallback((p: string) => api.localList(p), []);
  // Working as root (M3.3): every remote call below goes through the sudo session while this is on.
  const [sudo, setSudo] = useState(false);
  const sudoRef = useRef(false);
  sudoRef.current = sudo;
  const remoteLoader = useCallback((p: string) => api.sftpList(hostId, p, sudoRef.current), [hostId]);
  const local = usePane(localLoader, api.localHome);
  const remote = usePane(remoteLoader, () => api.sftpHome(hostId));
  const [dialog, setDialog] = useState<null | { kind: "mkdir" | "rename" | "delete" | "chmod"; entry?: FileEntry }>(null);
  const remoteRef = useRef(remote);
  remoteRef.current = remote;
  const localRef = useRef(local);
  localRef.current = local;

  const run = (dir: "up" | "down", source: string, destDir: string) => {
    const src: Endpoint = dir === "up" ? { kind: "local", path: source } : { kind: "remote", hostId, path: source, sudo: sudoRef.current };
    const dst: Endpoint = dir === "up" ? { kind: "remote", hostId, path: destDir, sudo: sudoRef.current } : { kind: "local", path: destDir };
    api.transferAdd(src, dst).catch(error);
  };

  // Refresh a pane when a transfer into the folder it shows finishes.
  const transfers = useApp((s) => s.transfers);
  const seenDone = useRef(new Set<string>());
  useEffect(() => {
    for (const t of transfers) {
      if (t.state !== "done" || seenDone.current.has(t.id)) continue;
      seenDone.current.add(t.id);
      const d = t.destDir;
      if (d.kind === "local" && d.path === localRef.current.path) localRef.current.refresh();
      if (d.kind === "remote" && d.hostId === hostId && d.path === remoteRef.current.path) remoteRef.current.refresh();
    }
  }, [transfers, hostId]);

  // A folder picked elsewhere (palette favourites).
  const goto = useApp((s) => s.sftpGoto);
  useEffect(() => {
    if (goto && goto.hostId === hostId) remoteRef.current.load(goto.path);
  }, [goto, hostId]);

  // Favourite folders on this host.
  const host = useApp((s) => s.hosts.find((h) => h.id === hostId));
  const favs = host?.favoritePaths ?? [];
  const isFav = !!remote.path && favs.includes(remote.path);
  const toggleFav = async () => {
    if (!host || !remote.path) return;
    const next = isFav ? favs.filter((p) => p !== remote.path) : [...favs, remote.path];
    try {
      await api.hostSave({ ...host, favoritePaths: next });
      await useApp.getState().refreshHosts();
    } catch (e) {
      error(e);
    }
  };
  const [copyTo, setCopyTo] = useState<FileEntry | null>(null);
  const openEditor = (e: FileEntry) => useApp.getState().openTab({ kind: "edit", title: e.name, hostId, path: e.path, sudo: sudoRef.current }, { unique: true });

  // The sudo password is asked for each new root session and never kept.
  const [pwPrompt, setPwPrompt] = useState<{ why: string; resolve: (pw: string | null) => void } | null>(null);
  const askPassword = (why: string) => new Promise<string | null>((resolve) => setPwPrompt({ why, resolve }));
  const toggleSudo = async () => {
    if (sudo) {
      await api.sftpSudoClose(hostId).catch(() => {});
      setSudo(false);
      sudoRef.current = false;
      remoteRef.current.refresh();
      return;
    }
    if (host?.production && !(await ask(`Browse and change files on the production host ${host.name} as root?`, { title: "Work as root on production?", kind: "warning", okLabel: "Use sudo", cancelLabel: "Cancel" }))) return;
    let password: string | undefined;
    let why = `sudo on ${host?.name ?? "this host"} asks for your password.`;
    for (;;) {
      try {
        await api.sftpSudoOpen(hostId, password);
        break;
      } catch (e) {
        const m = errMsg(e);
        if (!m.startsWith(NEED_PASSWORD) && m !== "sudo: wrong password") return error(e);
        const pw = await askPassword(m === "sudo: wrong password" ? "That password was wrong. Try again." : why);
        if (pw == null) return;
        password = pw;
        why = "";
      }
    }
    setSudo(true);
    sudoRef.current = true;
    remoteRef.current.refresh();
  };
  // The root session ended (e.g. the connection dropped): back to normal browsing.
  useEffect(() => {
    if (sudo && remote.err?.startsWith(NEED_PASSWORD)) {
      setSudo(false);
      toast("The root session ended; turn sudo on again to continue as root.", "error");
    }
  }, [remote.err, sudo, toast]);

  // Compress and extract on the server, as cancellable tasks.
  const [fileTask, setFileTask] = useState<{ label: string; taskId: string } | null>(null);
  const [compress, setCompress] = useState<FileEntry | null>(null);
  /** Runs a compress/extract call; asks for the sudo password when needed.
   * Returns null when it worked, otherwise the error message (already shown,
   * except "cancelled" and the "folder exists" answer, which callers handle). */
  const runFileTask = async (label: string, call: (password: string | undefined, taskId: string) => Promise<string>): Promise<string | null> => {
    let password: string | undefined;
    try {
      for (;;) {
        const taskId = `file:${hostId}:${Date.now()}`;
        setFileTask({ label, taskId });
        try {
          await call(password, taskId);
          toast(`${label}: done`);
          return null;
        } catch (e) {
          const m = errMsg(e);
          if (sudoRef.current && (m.startsWith(NEED_PASSWORD) || m === "sudo: wrong password")) {
            setFileTask(null);
            const pw = await askPassword(m === "sudo: wrong password" ? "That password was wrong. Try again." : `${label} runs as root; sudo asks for your password.`);
            if (pw == null) return "cancelled";
            password = pw;
            continue;
          }
          if (m !== "cancelled" && !m.startsWith(DEST_EXISTS)) error(e);
          return m;
        }
      }
    } finally {
      setFileTask(null);
      remoteRef.current.refresh();
    }
  };
  const extract = async (e: FileEntry) => {
    if (host?.production && !(await ask(`Extract ${e.name} on the production host ${host.name}?`, { title: "Extract on production?", kind: "warning", okLabel: "Extract", cancelLabel: "Cancel" }))) return;
    const err = await runFileTask(`Extract ${e.name}`, (pw, id) => api.remoteExtract(hostId, e.path, false, sudoRef.current, pw, id));
    if (!err?.startsWith(DEST_EXISTS)) return;
    const dest = err.slice(DEST_EXISTS.length);
    if (await ask(`${dest} already exists. Extract into it anyway? Files with the same names are replaced.`, { title: "Folder exists", kind: "warning", okLabel: "Extract into it", cancelLabel: "Cancel" })) {
      await runFileTask(`Extract ${e.name}`, (pw, id) => api.remoteExtract(hostId, e.path, true, sudoRef.current, pw, id));
    }
  };

  // Files open in the user's own editor; each save there is uploaded.
  const [externals, setExternals] = useState<LocalEditView[]>([]);
  useEffect(() => {
    api.localEditList().then((l) => setExternals(l.filter((x) => x.hostId === hostId))).catch(() => {});
    const un = listen<LocalEditView>("local-edit", (ev) => {
      const v = ev.payload;
      if (v.hostId !== hostId) return;
      setExternals((l) => (l.some((x) => x.id === v.id) ? l.map((x) => (x.id === v.id ? v : x)) : [...l, v]));
      if (v.state === "uploaded") useApp.getState().toast(`Uploaded ${v.remotePath.split("/").pop()}`);
      if (v.state === "uploaded" && parent(v.remotePath) === remoteRef.current.path) remoteRef.current.refresh();
    });
    return () => {
      un.then((f) => f());
    };
  }, [hostId]);
  const openExternal = async (e: FileEntry) => {
    try {
      await api.localEditOpen(hostId, e.path);
    } catch (err) {
      error(err);
    }
  };
  const stopExternal = (id: string) => api.localEditStop(id).then(() => setExternals((l) => l.filter((x) => x.id !== id)));
  const hostTransfers = transfers.filter((t) => touches(t, hostId));

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
      if (dialog.kind === "mkdir") await api.sftpMkdir(hostId, join(remote.path, value), sudo);
      if (dialog.kind === "rename" && dialog.entry) await api.sftpRename(hostId, dialog.entry.path, join(parent(dialog.entry.path), value), sudo);
      if (dialog.kind === "delete" && dialog.entry) await api.sftpRemove(hostId, dialog.entry.path, sudo);
      if (dialog.kind === "chmod" && dialog.entry) {
        const mode = parseInt(value, 8);
        if (Number.isNaN(mode)) throw new Error("Mode must be octal, e.g. 644");
        await api.sftpChmod(hostId, dialog.entry.path, mode, sudo);
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
          pane={remote}
          onOpen={(e) => (e.isDir ? remote.load(e.path) : openEditor(e))}
          dragKind="remote"
          title={sudo ? "Root" : "Remote"}
          header={
            <>
              <IconButton title={sudo ? "Stop working as root" : "Work as root (sudo)"} onClick={toggleSudo}>
                <ShieldAlert size={14} className={sudo ? "text-danger" : ""} />
              </IconButton>
              <IconButton title={isFav ? "Remove from favourites" : "Add this folder to favourites"} disabled={!remote.path} onClick={toggleFav}>
                <Star size={14} className={isFav ? "fill-warn text-warn" : ""} />
              </IconButton>
              {favs.length > 0 && (
                <Select className="h-6! w-32 px-2 py-0 text-xs" value="" onChange={(e) => e.target.value && remote.load(e.target.value)} title="Favourite folders">
                  <option value="">Favourites…</option>
                  {favs.map((p) => (
                    <option key={p} value={p}>
                      {p}
                    </option>
                  ))}
                </Select>
              )}
            </>
          }
          onDropFrom={(kind, path) => kind === "local" && run("up", path, remote.path)}
          actions={
            <>
              <Button size="sm" variant="primary" disabled={!remoteSel || !local.path} onClick={() => remoteSel && run("down", remoteSel.path, local.path)}>
                <ArrowDownToLine size={13} /> Download
              </Button>
              <IconButton title="Edit" disabled={!remoteSel || remoteSel.isDir} onClick={() => remoteSel && openEditor(remoteSel)}>
                <FilePen size={14} />
              </IconButton>
              <IconButton title={sudo ? "Open in my editor isn't available as root" : "Open in my editor (uploads each save)"} disabled={!remoteSel || remoteSel.isDir || sudo} onClick={() => remoteSel && openExternal(remoteSel)}>
                <ExternalLink size={14} />
              </IconButton>
              <IconButton title="Compress…" disabled={!remoteSel} onClick={() => remoteSel && setCompress(remoteSel)}>
                <Archive size={14} />
              </IconButton>
              <IconButton title="Extract here (into a new folder)" disabled={!remoteSel || remoteSel.isDir || !ARCHIVE.test(remoteSel.name)} onClick={() => remoteSel && extract(remoteSel)}>
                <PackageOpen size={14} />
              </IconButton>
              <IconButton title="Copy to another host…" disabled={!remoteSel} onClick={() => remoteSel && setCopyTo(remoteSel)}>
                <ArrowLeftRight size={14} />
              </IconButton>
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
      {fileTask && (
        <div className="flex shrink-0 items-center gap-3 border-t border-border bg-surface-1 px-3 py-1.5 text-xs">
          <RefreshCw size={12} className="animate-spin text-accent" />
          <span>{fileTask.label}…</span>
          <div className="flex-1" />
          <button className="cursor-pointer text-fg-muted hover:text-fg" onClick={() => api.taskCancel(fileTask.taskId)}>
            Cancel
          </button>
        </div>
      )}
      {externals.length > 0 && (
        <div className="shrink-0 border-t border-border bg-surface-1 text-xs">
          {externals.map((x) => (
            <div key={x.id} className="flex items-center gap-3 px-3 py-1">
              <ExternalLink size={12} className="text-accent" />
              <span className="font-mono">{x.remotePath}</span>
              <span className={x.state === "conflict" || x.state === "error" ? "text-danger" : "text-fg-subtle"} title={x.message ?? undefined}>
                {x.state === "watching"
                  ? "open in your editor; each save is uploaded"
                  : x.state === "uploading"
                    ? "uploading…"
                    : x.state === "uploaded"
                      ? "uploaded"
                      : x.state === "conflict"
                        ? "not uploaded: changed on the server since"
                        : `not uploaded: ${x.message}`}
              </span>
              {x.state === "conflict" && (
                <button className="cursor-pointer text-accent hover:underline" onClick={() => api.localEditPush(x.id, true)}>
                  Upload anyway
                </button>
              )}
              <div className="flex-1" />
              <IconButton title="Stop (removes the local copy)" className="h-6 w-6" onClick={() => stopExternal(x.id)}>
                <X size={12} />
              </IconButton>
            </div>
          ))}
        </div>
      )}
      {hostTransfers.length > 0 && (
        <div className="max-h-40 overflow-y-auto border-t border-border bg-surface-1">
          <div className="flex items-center justify-between px-3 py-1 text-xs text-fg-subtle">
            <span>Transfers</span>
            <span className="flex gap-3">
              <button className="cursor-pointer hover:text-fg" onClick={() => useApp.getState().setTransfersOpen(true)}>
                All transfers
              </button>
              <button
                className="cursor-pointer hover:text-fg"
                onClick={() => {
                  const ids = hostTransfers.filter((t) => ["done", "failed", "cancelled"].includes(t.state)).map((t) => t.id);
                  api.transferClear(ids).then(() => useApp.getState().setTransfers(useApp.getState().transfers.filter((t) => !ids.includes(t.id))));
                }}
              >
                Clear finished
              </button>
            </span>
          </div>
          <TransferList filter={(t) => touches(t, hostId)} />
        </div>
      )}
      {compress && (
        <CompressDialog
          entry={compress}
          onClose={() => setCompress(null)}
          onSubmit={(archive, format) => {
            const e = compress;
            setCompress(null);
            runFileTask(`Compress ${e.name}`, (pw, id) => api.remoteCompress(hostId, parent(e.path), [e.name], archive, format, sudoRef.current, pw, id));
          }}
        />
      )}
      {pwPrompt && (
        <SudoPasswordDialog
          why={pwPrompt.why}
          onDone={(pw) => {
            pwPrompt.resolve(pw);
            setPwPrompt(null);
          }}
        />
      )}
      {copyTo && <CopyToHostDialog fromHostId={hostId} entry={copyTo} onClose={() => setCopyTo(null)} />}
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
  header,
}: {
  header?: React.ReactNode;
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
        {header}
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

/** Copy a file or folder from this host to another (streamed through Shellport). */
function CopyToHostDialog({ fromHostId, entry, onClose }: { fromHostId: string; entry: FileEntry; onClose: () => void }) {
  const hosts = useApp((s) => s.hosts);
  const { error } = useApp();
  const others = hosts.filter((h) => h.id !== fromHostId);
  const [to, setTo] = useState(others[0]?.id ?? fromHostId);
  const [dir, setDir] = useState("");
  useEffect(() => {
    setDir("");
    api.sftpHome(to).then(setDir).catch(() => setDir("."));
  }, [to]);
  const target = hosts.find((h) => h.id === to);
  const go = async () => {
    if (target?.production) {
      const ok = await ask(`Copy "${entry.name}" to ${dir} on the production host ${target.name}?`, {
        title: "Copy to production?",
        kind: "warning",
        okLabel: "Copy",
        cancelLabel: "Cancel",
      });
      if (!ok) return;
    }
    try {
      await api.transferAdd({ kind: "remote", hostId: fromHostId, path: entry.path }, { kind: "remote", hostId: to, path: dir });
      useApp.getState().toast(`Copying ${entry.name} to ${target?.name ?? "host"}`);
      onClose();
    } catch (e) {
      error(e);
    }
  };
  return (
    <Modal
      title={`Copy "${entry.name}" to another host`}
      onClose={onClose}
      width="max-w-md"
      footer={
        <Button variant="primary" disabled={!dir} onClick={go}>
          Copy
        </Button>
      }
    >
      <div className="space-y-3">
        <Field label="To host">
          <Select value={to} onChange={(e) => setTo(e.target.value)}>
            {[...others, ...hosts.filter((h) => h.id === fromHostId)].map((h) => (
              <option key={h.id} value={h.id}>
                {h.name}
                {h.id === fromHostId ? " (same host)" : ""}
                {h.production ? " · PROD" : ""}
              </option>
            ))}
          </Select>
        </Field>
        <Field label="Into folder" hint="The data goes through Shellport; nothing is stored on this computer.">
          <Input className="font-mono text-xs" value={dir} onChange={(e) => setDir(e.target.value)} placeholder="/home/user" />
        </Field>
      </div>
    </Modal>
  );
}

/** Asks for the sudo password; it goes straight to sudo and isn't stored. */
function SudoPasswordDialog({ why, onDone }: { why: string; onDone: (pw: string | null) => void }) {
  const [pw, setPw] = useState("");
  return (
    <Modal
      title="sudo password"
      onClose={() => onDone(null)}
      width="max-w-sm"
      footer={
        <Button variant="primary" disabled={!pw} onClick={() => onDone(pw)}>
          Continue
        </Button>
      }
    >
      <form
        className="space-y-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (pw) onDone(pw);
        }}
      >
        {why && <p className="text-sm">{why}</p>}
        <Input autoFocus type="password" value={pw} onChange={(e) => setPw(e.target.value)} placeholder="Your password on the server" />
        <p className="text-xs text-fg-subtle">Sent once to sudo for this session; Shellport doesn't store it.</p>
      </form>
    </Modal>
  );
}

/** Name and format for a new archive. */
function CompressDialog({ entry, onClose, onSubmit }: { entry: FileEntry; onClose: () => void; onSubmit: (archive: string, format: "targz" | "zip") => void }) {
  const [format, setFormat] = useState<"targz" | "zip">("targz");
  const [name, setName] = useState(`${entry.name}.tar.gz`);
  const pick = (f: "targz" | "zip") => {
    setFormat(f);
    setName((n) => n.replace(/\.(tar\.gz|zip)$/, "") + (f === "zip" ? ".zip" : ".tar.gz"));
  };
  return (
    <Modal
      title={`Compress "${entry.name}"`}
      onClose={onClose}
      width="max-w-md"
      footer={
        <Button variant="primary" disabled={!name || name.includes("/")} onClick={() => onSubmit(name, format)}>
          Compress
        </Button>
      }
    >
      <div className="space-y-3">
        <Field label="Format">
          <Select value={format} onChange={(e) => pick(e.target.value as "targz" | "zip")}>
            <option value="targz">.tar.gz</option>
            <option value="zip">.zip</option>
          </Select>
        </Field>
        <Field label="Archive name" hint="Created next to the original, on the server.">
          <Input className="font-mono text-xs" value={name} onChange={(e) => setName(e.target.value)} />
        </Field>
      </div>
    </Modal>
  );
}

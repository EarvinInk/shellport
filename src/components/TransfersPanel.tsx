import clsx from "clsx";
import { ArrowDownToLine, ArrowLeftRight, ArrowUpFromLine, Pause, Play, RotateCcw, X, Zap } from "lucide-react";
import { api, errMsg, formatBytes } from "../lib/ipc";
import type { Endpoint, TransferView } from "../lib/types";
import { useApp } from "../store";
import { IconButton, Modal } from "./ui";

/** "42s", "3m 05s", "1h 02m". */
function formatEta(secs: number): string {
  const p = (n: number) => String(n).padStart(2, "0");
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m ${p(secs % 60)}s`;
  return `${Math.floor(secs / 3600)}h ${p(Math.floor(secs / 60) % 60)}m`;
}

const finished = (t: TransferView) => t.state === "done" || t.state === "failed" || t.state === "cancelled";

/** Running, queued or paused transfers. */
export function activeTransfers(ts: TransferView[]): number {
  return ts.filter((t) => !finished(t)).length;
}

function useEndpointLabel() {
  const hosts = useApp((s) => s.hosts);
  return (e: Endpoint) => (e.kind === "local" ? `local:${e.path}` : `${hosts.find((h) => h.id === e.hostId)?.name ?? "?"}:${e.path}`);
}

function act(p: Promise<unknown>) {
  p.catch((e) => useApp.getState().toast(errMsg(e), "error"));
}

/** Transfers from the app-wide queue; `filter` narrows it (e.g. to one host). */
export function TransferList({ filter, empty }: { filter?: (t: TransferView) => boolean; empty?: React.ReactNode }) {
  const all = useApp((s) => s.transfers);
  const label = useEndpointLabel();
  const list = (filter ? all.filter(filter) : all).slice().reverse();
  if (!list.length) return <>{empty ?? null}</>;
  return (
    <div>
      {list.map((t) => {
        const dir = t.source.kind === "local" ? "up" : t.destDir.kind === "local" ? "down" : "copy";
        const pct = t.state === "done" ? 100 : t.total ? Math.min(100, (t.bytes / t.total) * 100) : 0;
        return (
          <div key={t.id} className="flex items-center gap-3 px-3 py-1.5 text-xs" title={`${label(t.source)} → ${label(t.destDir)}`}>
            {dir === "up" ? <ArrowUpFromLine size={12} /> : dir === "down" ? <ArrowDownToLine size={12} /> : <ArrowLeftRight size={12} />}
            <div className="w-56 min-w-0">
              <div className="truncate">{t.name}</div>
              <div className="truncate text-[10px] text-fg-subtle">→ {label(t.destDir)}</div>
            </div>
            <div className="h-1.5 flex-1 overflow-hidden rounded bg-surface-3">
              <div
                className={clsx(
                  "h-full transition-[width]",
                  t.state === "failed" || t.state === "cancelled" ? "bg-danger" : t.state === "done" ? "bg-ok" : t.state === "paused" ? "bg-warn" : "bg-accent",
                )}
                style={{ width: `${pct}%` }}
              />
            </div>
            <span className="w-72 truncate text-right text-fg-muted">
              <Status t={t} />
            </span>
            <span className="flex min-w-16 shrink-0 justify-end gap-0.5">
              {t.state === "failed" && !!t.wouldOverwrite && (
                <button className="h-6 cursor-pointer rounded px-2 text-xs text-accent hover:bg-surface-3" onClick={() => act(api.transferResume(t.id, true))}>
                  Overwrite
                </button>
              )}
              {t.state === "queued" && (
                <IconButton title="Start now" className="h-6 w-6" onClick={() => act(api.transferStartNow(t.id))}>
                  <Zap size={12} />
                </IconButton>
              )}
              {(t.state === "running" || t.state === "queued") && (
                <IconButton title="Pause" className="h-6 w-6" onClick={() => act(api.transferPause(t.id))}>
                  <Pause size={12} />
                </IconButton>
              )}
              {t.state === "paused" && (
                <IconButton title="Resume" className="h-6 w-6" onClick={() => act(api.transferResume(t.id))}>
                  <Play size={12} />
                </IconButton>
              )}
              {t.state === "failed" && !t.wouldOverwrite && (
                <IconButton title="Retry (continues where it stopped)" className="h-6 w-6" onClick={() => act(api.transferResume(t.id))}>
                  <RotateCcw size={12} />
                </IconButton>
              )}
              {(!finished(t) || (t.state === "failed" && !t.wouldOverwrite)) && (
                <IconButton title="Cancel (removes the partly copied file)" className="h-6 w-6" onClick={() => act(api.transferCancel(t.id))}>
                  <X size={12} />
                </IconButton>
              )}
            </span>
          </div>
        );
      })}
    </div>
  );
}

function Status({ t }: { t: TransferView }) {
  const size = `${formatBytes(t.bytes)} / ${formatBytes(t.total)}`;
  const files = t.filesTotal > 1 ? ` · ${t.filesDone}/${t.filesTotal} files` : "";
  switch (t.state) {
    case "queued":
      return <>Waiting{t.total ? ` · ${size}` : ""}</>;
    case "running":
      return (
        <>
          {size}
          {files}
          {t.speed > 0 && ` · ${formatBytes(t.speed)}/s`}
          {t.etaSecs != null && t.speed > 0 && ` · ${formatEta(t.etaSecs)} left`}
        </>
      );
    case "paused":
      return <span className="text-warn">Paused · {size}</span>;
    case "done":
      return (
        <>
          Done · {formatBytes(t.total)}
          {t.filesTotal > 1 ? ` · ${t.filesTotal} files` : ""}
        </>
      );
    case "cancelled":
      return <span className="text-warn">Cancelled</span>;
    case "failed":
      if (t.wouldOverwrite)
        return (
          <span className="text-warn" title={t.error ?? undefined}>
            {t.wouldOverwrite} file{t.wouldOverwrite === 1 ? "" : "s"} already there
          </span>
        );
      return (
        <span className="text-danger" title={t.error ?? undefined}>
          Failed: {t.error}
        </span>
      );
  }
}

/** The app-wide Transfers panel (status-bar button, palette). */
export function TransfersPanel() {
  const open = useApp((s) => s.transfersOpen);
  const setOpen = useApp((s) => s.setTransfersOpen);
  const ts = useApp((s) => s.transfers);
  if (!open) return null;
  const done = ts.filter(finished).length;
  return (
    <Modal
      title="Transfers"
      onClose={() => setOpen(false)}
      width="max-w-4xl"
      footer={
        <button className="cursor-pointer text-xs text-fg-muted hover:text-fg disabled:opacity-40" disabled={!done} onClick={() => act(api.transferClear().then(() => useApp.getState().setTransfers(ts.filter((t) => !finished(t)))))}>
          Clear finished
        </button>
      }
    >
      <div className="-mx-3 max-h-[60vh] overflow-y-auto">
        <TransferList empty={<p className="px-3 py-6 text-center text-sm text-fg-subtle">No transfers yet. Upload, download or copy files from a host's file browser.</p>} />
      </div>
    </Modal>
  );
}

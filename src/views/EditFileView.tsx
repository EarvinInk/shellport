import { ask } from "@tauri-apps/plugin-dialog";
import { AlertTriangle, Download, RefreshCw, Save, WrapText } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { Editor } from "../components/Editor";
import { Button, IconButton, ProdBadge } from "../components/ui";
import { api, errMsg } from "../lib/ipc";
import { setCloseGuard, useApp } from "../store";

/** Must match `remote_edit::CHANGED_PREFIX` in the backend. */
const CHANGED_PREFIX = "changed on server:";

const nameOf = (p: string) => p.split("/").filter(Boolean).pop() ?? p;

/** A remote text file in the shared editor (M3.2). */
export function EditFileView({ tabId, hostId, path, sudo, active }: { tabId: string; hostId: string; path: string; sudo: boolean; active: boolean }) {
  const host = useApp((s) => s.hosts.find((h) => h.id === hostId));
  const { toast, renameTab } = useApp();
  const [text, setText] = useState("");
  const [saved, setSaved] = useState<string | null>(null);
  const [base, setBase] = useState<{ size: number; mtime: number | null }>({ size: 0, mtime: null });
  const [readOnly, setReadOnly] = useState(false);
  const [loadErr, setLoadErr] = useState<string | null>(null);
  const [saveErr, setSaveErr] = useState<string | null>(null);
  const [conflict, setConflict] = useState(false);
  const [busy, setBusy] = useState(false);
  const [wrap, setWrap] = useState(false);
  const dirty = saved !== null && text !== saved;

  const load = useCallback(async () => {
    setLoadErr(null);
    setConflict(false);
    setSaveErr(null);
    try {
      const f = await api.sftpReadText(hostId, path, sudo);
      setText(f.text);
      setSaved(f.text);
      setBase({ size: f.size, mtime: f.mtime ?? null });
      setReadOnly(f.readOnly);
    } catch (e) {
      setLoadErr(errMsg(e));
    }
  }, [hostId, path, sudo]);
  useEffect(() => {
    load();
  }, [load]);

  const save = useCallback(
    async (force = false) => {
      if (readOnly || busy || saved === null) return;
      setBusy(true);
      setSaveErr(null);
      try {
        const r = await api.sftpWriteText(hostId, path, text, base.size, base.mtime, force, sudo);
        setBase({ size: r.size, mtime: r.mtime ?? null });
        setSaved(text);
        setConflict(false);
        toast(`Saved ${nameOf(path)}`);
      } catch (e) {
        const m = errMsg(e);
        if (m.startsWith(CHANGED_PREFIX)) setConflict(true);
        else setSaveErr(m);
      } finally {
        setBusy(false);
      }
    },
    [hostId, path, text, base, readOnly, busy, saved, toast, sudo],
  );

  const saveCopy = async () => {
    try {
      const p = await api.textSave("Save a copy", nameOf(path), text);
      if (p) toast(`Saved a copy to ${p}`);
    } catch (e) {
      toast(errMsg(e), "error");
    }
  };

  const reload = async () => {
    if (dirty && !(await ask(`Reload ${nameOf(path)} from the server? Your unsaved changes are lost.`, { title: "Reload?", kind: "warning", okLabel: "Reload" }))) return;
    load();
  };

  // A dot on the tab while there are unsaved changes; ask before closing then.
  useEffect(() => {
    renameTab(tabId, `${dirty ? "● " : ""}${nameOf(path)}`);
    setCloseGuard(tabId, dirty ? () => ask(`Close ${nameOf(path)} without saving your changes?`, { title: "Unsaved changes", kind: "warning", okLabel: "Close without saving" }) : null);
  }, [dirty, tabId, path, renameTab]);
  useEffect(() => () => setCloseGuard(tabId, null), [tabId]);

  const saveRef = useRef(save);
  saveRef.current = save;

  return (
    <div className="flex h-full flex-col bg-surface-0" data-active={active || undefined}>
      <div className="flex shrink-0 items-center gap-2 border-b border-border px-3 py-1.5 text-xs">
        <span className="font-medium text-fg">{host?.name ?? "?"}</span>
        {host?.production && <ProdBadge />}
        {sudo && <span className="rounded bg-danger/20 px-1.5 text-[10px] font-semibold tracking-wide text-danger">ROOT</span>}
        <span className="truncate font-mono text-fg-muted" title={path}>
          {path}
        </span>
        {readOnly && <span className="rounded bg-warn/15 px-1.5 text-warn">read-only: not UTF-8 text</span>}
        <div className="flex-1" />
        <IconButton title={wrap ? "Don't wrap long lines" : "Wrap long lines"} onClick={() => setWrap((w) => !w)}>
          <WrapText size={14} className={wrap ? "text-accent" : ""} />
        </IconButton>
        <IconButton title="Reload from the server" onClick={reload}>
          <RefreshCw size={14} />
        </IconButton>
        <IconButton title="Save a copy on this computer…" disabled={saved === null} onClick={saveCopy}>
          <Download size={14} />
        </IconButton>
        <Button size="sm" variant="primary" disabled={!dirty || readOnly || busy} onClick={() => save()}>
          <Save size={13} /> Save
        </Button>
      </div>
      {conflict && (
        <div className="flex shrink-0 items-center gap-3 border-b border-warn/40 bg-warn/10 px-3 py-2 text-xs text-warn">
          <AlertTriangle size={14} />
          <span className="flex-1">{nameOf(path)} was changed on the server after you opened it. Saving now would replace those changes.</span>
          <Button size="sm" variant="danger" onClick={() => save(true)}>
            Overwrite
          </Button>
          <Button size="sm" onClick={reload}>
            Reload (lose my changes)
          </Button>
          <Button size="sm" onClick={saveCopy}>
            Save a copy…
          </Button>
        </div>
      )}
      {saveErr && (
        <div className="flex shrink-0 items-center gap-3 border-b border-danger/40 bg-danger/10 px-3 py-2 text-xs text-danger">
          <span className="flex-1">Couldn't save: {saveErr}</span>
          <Button size="sm" onClick={saveCopy}>
            Save a copy…
          </Button>
        </div>
      )}
      {loadErr ? (
        <div className="m-4 rounded bg-danger/10 p-3 text-sm text-danger">
          {loadErr}
          <div className="mt-2">
            <Button size="sm" onClick={load}>
              Try again
            </Button>
          </div>
        </div>
      ) : saved === null ? (
        <div className="p-4 text-sm text-fg-subtle">Opening {nameOf(path)}…</div>
      ) : (
        <Editor value={text} onChange={setText} fileName={path} readOnly={readOnly} wrap={wrap} autoFocus onSave={() => saveRef.current()} className="min-h-0 flex-1" />
      )}
    </div>
  );
}

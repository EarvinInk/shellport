import clsx from "clsx";
import {
  ArrowDown,
  ArrowUp,
  ChevronDown,
  ChevronRight,
  Columns3,
  Copy,
  Database,
  Eye,
  Folder,
  FunctionSquare,
  Layers,
  Loader2,
  Lock,
  PanelLeftClose,
  PanelLeftOpen,
  Pencil,
  Play,
  RefreshCw,
  Search,
  ShieldAlert,
  Square,
  Table2,
  Unplug,
  X,
} from "lucide-react";
import { ask } from "@tauri-apps/plugin-dialog";
import { createContext, useCallback, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Button, Empty, IconButton, Modal, ProdBadge, Select, TextArea } from "../components/ui";
import { DB_ENGINES } from "../lib/dbEngines";
import { api, errMsg } from "../lib/ipc";
import type { Cell, DbConnected, QueryOutput, QueryRisk, SchemaNode, StatementResult, TxState } from "../lib/types";
import { setCloseGuard, useApp } from "../store";

const ROW_LIMITS = [100, 1000, 5000, 20000];

// ---------- small persistence helpers (per-viewer conveniences only) ----------

function readStr(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}
function writeStr(key: string, v: string) {
  try {
    localStorage.setItem(key, v);
  } catch {
    /* storage unavailable: keep in memory only */
  }
}
function readNum(key: string, fallback: number): number {
  const n = Number(readStr(key));
  return Number.isFinite(n) && n > 0 ? n : fallback;
}

const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));

/** Drag helper: calls `onMove` with the pointer delta since mousedown. */
function startDrag(e: React.MouseEvent, cursor: string, onMove: (dx: number, dy: number) => void, onEnd?: () => void) {
  e.preventDefault();
  const x0 = e.clientX;
  const y0 = e.clientY;
  const prev = document.body.style.cursor;
  document.body.style.cursor = cursor;
  document.body.style.userSelect = "none";
  const move = (ev: MouseEvent) => onMove(ev.clientX - x0, ev.clientY - y0);
  const up = () => {
    window.removeEventListener("mousemove", move);
    window.removeEventListener("mouseup", up);
    document.body.style.cursor = prev;
    document.body.style.userSelect = "";
    onEnd?.();
  };
  window.addEventListener("mousemove", move);
  window.addEventListener("mouseup", up);
}

// ---------- the view ----------

interface MenuState {
  x: number;
  y: number;
  node: SchemaNode;
}

export function DbView({ tabId, dbId, active }: { tabId: string; dbId: string; active: boolean }) {
  const profile = useApp((s) => s.dbs.find((d) => d.id === dbId));
  const { error, toast, setDbEditor, setUnlockPrompt, dbLiveAdd, dbTxAdd } = useApp.getState();
  const vaultUnlocked = useApp((s) => s.vault.unlocked);
  const [conn, setConn] = useState<DbConnected | null>(null);
  const [connErr, setConnErr] = useState<string | null>(null);
  const [connecting, setConnecting] = useState(false);
  const sessionRef = useRef<string | null>(null);
  /** Bumped on every (re)connect and on unmount, so a late reply is dropped. */
  const genRef = useRef(0);
  const engine = profile ? DB_ENGINES[profile.engine] : undefined;

  // Each tab keeps its own draft; older versions kept one per database.
  const draftKey = `shellport.dbsql.tab.${tabId}`;
  const [sql, setSql] = useState(() => readStr(draftKey) ?? readStr(`shellport.dbsql.${dbId}`) ?? engine?.sampleQuery ?? "");
  const [tx, setTx] = useState<TxState>("idle");
  const [confirm, setConfirm] = useState<{ sql: string; risks: QueryRisk[] } | null>(null);
  /** Shown after an automatic reconnect: the last query did not run again. */
  const [lostNotice, setLostNotice] = useState<string | null>(null);
  const autoReconnectedRef = useRef(false);
  const [running, setRunning] = useState(false);
  const [output, setOutput] = useState<QueryOutput | null>(null);
  const [queryErr, setQueryErr] = useState<string | null>(null);
  const [resultIdx, setResultIdx] = useState(0);
  const [rowLimit, setRowLimit] = useState(1000);
  const [treeKey, setTreeKey] = useState(0);
  const [filter, setFilter] = useState("");
  const [menu, setMenu] = useState<MenuState | null>(null);
  const editorRef = useRef<HTMLTextAreaElement>(null);
  const resultsRef = useRef<HTMLDivElement>(null);

  // Layout: tree width/visibility and editor height, remembered for all DB tabs.
  const [treeOpen, setTreeOpen] = useState(() => readStr("shellport.db.treeOpen") !== "0");
  const [treeW, setTreeW] = useState(() => clamp(readNum("shellport.db.treeWidth", 256), 180, 520));
  const [editorH, setEditorH] = useState(() => readNum("shellport.db.editorHeight", 200));
  const mainRef = useRef<HTMLDivElement>(null);
  useEffect(() => writeStr("shellport.db.treeOpen", treeOpen ? "1" : "0"), [treeOpen]);

  /** Track this tab in the sidebar's "connected" indicator. */
  const liveRef = useRef(false);
  const setLive = useCallback(
    (on: boolean) => {
      if (on !== liveRef.current) {
        liveRef.current = on;
        dbLiveAdd(dbId, on ? 1 : -1);
      }
    },
    [dbId, dbLiveAdd],
  );

  const connect = useCallback(async () => {
    if (sessionRef.current) {
      api.dbDisconnect(sessionRef.current).catch(() => {});
      sessionRef.current = null;
    }
    setLive(false);
    const gen = ++genRef.current;
    setConn(null);
    setConnErr(null);
    setConnecting(true);
    try {
      const c = await api.dbConnect(dbId);
      if (gen !== genRef.current) {
        api.dbDisconnect(c.sessionId).catch(() => {});
        return;
      }
      sessionRef.current = c.sessionId;
      setConn(c);
      setLive(true);
      setTreeKey((k) => k + 1);
    } catch (e) {
      if (gen !== genRef.current) return;
      const msg = errMsg(e);
      setConnErr(msg);
      // Shown in the tab itself; only the vault case needs the global prompt.
      if (/unlock the vault/i.test(msg)) setUnlockPrompt(true);
    } finally {
      if (gen === genRef.current) setConnecting(false);
    }
  }, [dbId, setLive, setUnlockPrompt]);

  useEffect(() => {
    connect();
    return () => {
      genRef.current++;
      if (sessionRef.current) api.dbDisconnect(sessionRef.current).catch(() => {});
      sessionRef.current = null;
      setLive(false);
    };
  }, [connect, setLive]);

  // Waiting on the vault: reconnect as soon as it is unlocked.
  useEffect(() => {
    if (vaultUnlocked && connErr && /unlock the vault/i.test(connErr)) connect();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [vaultUnlocked]);

  useEffect(() => {
    const t = setTimeout(() => writeStr(draftKey, sql), 500);
    return () => clearTimeout(t);
  }, [draftKey, sql]);

  useEffect(() => {
    if (active && conn) editorRef.current?.focus();
  }, [active, conn]);

  // Open transactions: count them for the sidebar and guard closing the tab.
  useEffect(() => {
    if (tx === "idle") return;
    dbTxAdd(dbId, 1);
    setCloseGuard(tabId, () =>
      ask("This tab has an open transaction. Closing it rolls back the uncommitted changes.", {
        title: "Close and roll back?",
        kind: "warning",
        okLabel: "Close and roll back",
        cancelLabel: "Keep tab open",
      }),
    );
    return () => {
      dbTxAdd(dbId, -1);
      setCloseGuard(tabId, null);
    };
  }, [tx, dbId, tabId, dbTxAdd]);

  /** Reconnect, asking first when that would roll back an open transaction. */
  const reconnect = async () => {
    if (tx !== "idle" && !(await ask("Reconnecting rolls back the open transaction.", { title: "Reconnect?", kind: "warning", okLabel: "Reconnect" }))) return;
    setTx("idle");
    setLostNotice(null);
    connect();
  };

  /** Finish the open transaction without touching the results on screen. */
  const endTx = async (verb: "COMMIT" | "ROLLBACK") => {
    if (!conn) return;
    try {
      const out = await api.dbQuery(conn.sessionId, verb, 1, true);
      if (out.kind === "done") setTx(out.output.tx);
      toast(verb === "COMMIT" ? "Transaction committed" : "Transaction rolled back", verb === "COMMIT" ? "success" : "info");
    } catch (e) {
      error(e);
      api.dbTxState(conn.sessionId).then(setTx).catch(() => {});
    }
  };

  const run = async (text?: string, confirmed = false) => {
    if (!conn || running) return;
    const el = editorRef.current;
    const selected = el && el.selectionStart !== el.selectionEnd ? el.value.slice(el.selectionStart, el.selectionEnd) : "";
    const toRun = (text ?? (selected || sql)).trim();
    if (!toRun) return;
    setRunning(true);
    setQueryErr(null);
    setLostNotice(null);
    try {
      const res = await api.dbQuery(conn.sessionId, toRun, rowLimit, confirmed);
      if (res.kind === "needsConfirmation") {
        setConfirm({ sql: toRun, risks: res.risks });
        return;
      }
      const out = res.output;
      autoReconnectedRef.current = false;
      setOutput(out);
      setTx(out.tx);
      // Show the last statement that returned rows, else the last one.
      const withRows = out.results.map((r, i) => (r.columns.length ? i : -1)).filter((i) => i >= 0);
      setResultIdx(withRows.length ? withRows[withRows.length - 1] : Math.max(0, out.results.length - 1));
    } catch (e) {
      const msg = errMsg(e);
      setOutput(null);
      if (/^connection lost/i.test(msg)) {
        // The server or network went away. Reconnect once on our own, but
        // never re-run the query: it may have been a write.
        setTx("idle");
        if (!autoReconnectedRef.current) {
          autoReconnectedRef.current = true;
          setLostNotice(toRun);
          connect();
        } else {
          setConn(null);
          setLive(false);
          setConnErr(msg);
        }
      } else {
        setQueryErr(msg);
        api.dbTxState(conn.sessionId).then(setTx).catch(() => {});
      }
    } finally {
      setRunning(false);
    }
  };

  const cancel = () => {
    if (conn) api.dbCancel(conn.sessionId).catch(error);
  };

  const preview = (n: SchemaNode) => {
    if (!n.previewSql) return;
    setSql(n.previewSql);
    run(n.previewSql);
  };

  /** Insert text at the editor's cursor (replacing any selection). */
  const insert = (text: string) => {
    const el = editorRef.current;
    const a = el?.selectionStart ?? sql.length;
    const b = el?.selectionEnd ?? sql.length;
    setSql(sql.slice(0, a) + text + sql.slice(b));
    requestAnimationFrame(() => {
      el?.focus();
      el?.setSelectionRange(a + text.length, a + text.length);
    });
  };

  const treeCtx = useMemo(
    () => ({
      sessionId: conn?.sessionId ?? "",
      filter: filter.trim().toLowerCase(),
      onPreview: preview,
      onMenu: (e: React.MouseEvent, node: SchemaNode) => {
        e.preventDefault();
        setMenu({ x: e.clientX, y: e.clientY, node });
      },
    }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [conn?.sessionId, filter, rowLimit, running],
  );

  if (!profile) return <Empty title="Database connection not found">It may have been deleted.</Empty>;

  const current = output?.results[resultIdx];
  const lineCount = sql.split("\n").length;

  return (
    <div className="flex h-full min-h-0">
      {/* Schema tree */}
      {treeOpen && (
        <>
          <aside className="flex shrink-0 flex-col bg-surface-1" style={{ width: treeW }}>
            <div className="flex h-10 items-center gap-1 border-b border-border px-2">
              <Database size={14} className="ml-1 shrink-0 text-fg-muted" />
              <div className="min-w-0 flex-1 truncate text-xs font-medium" title={conn?.serverVersion}>
                {profile.database || profile.username || profile.name}
              </div>
              <IconButton title="Reconnect" onClick={reconnect} disabled={connecting}>
                <RefreshCw size={13} className={clsx(connecting && "animate-spin")} />
              </IconButton>
              <IconButton title="Hide schema" onClick={() => setTreeOpen(false)}>
                <PanelLeftClose size={14} />
              </IconButton>
            </div>
            {conn && (
              <div className="relative border-b border-border px-2 py-1.5">
                <Search size={12} className="pointer-events-none absolute left-4 top-1/2 -translate-y-1/2 text-fg-subtle" />
                <input
                  value={filter}
                  onChange={(e) => setFilter(e.target.value)}
                  onKeyDown={(e) => e.key === "Escape" && setFilter("")}
                  placeholder="Filter tables, views, functions"
                  className="h-7 w-full rounded-md border border-border bg-surface-0 pl-7 pr-6 text-xs text-fg outline-none placeholder:text-fg-subtle focus:border-accent"
                />
                {filter && (
                  <button className="absolute right-3.5 top-1/2 -translate-y-1/2 text-fg-subtle hover:text-fg cursor-pointer" onClick={() => setFilter("")} aria-label="Clear filter">
                    <X size={12} />
                  </button>
                )}
              </div>
            )}
            <div className="min-h-0 flex-1 overflow-auto py-1 text-xs" data-tree role="tree">
              {connecting && (
                <div className="flex items-center gap-2 px-3 py-2 text-fg-subtle">
                  <Loader2 size={13} className="animate-spin" /> Connecting…
                </div>
              )}
              {!connecting && !conn && <div className="px-3 py-2 text-fg-subtle">Not connected</div>}
              {conn && (
                <TreeCtx.Provider value={treeCtx}>
                  <TreeLevel key={treeKey} path={[]} depth={0} />
                </TreeCtx.Provider>
              )}
            </div>
          </aside>
          <div
            role="separator"
            aria-orientation="vertical"
            title="Drag to resize"
            className="w-1 shrink-0 cursor-col-resize border-r border-border bg-surface-1 hover:bg-accent/40"
            onMouseDown={(e) => {
              const w0 = treeW;
              startDrag(
                e,
                "col-resize",
                (dx) => setTreeW(clamp(w0 + dx, 180, 520)),
                () => setTreeW((w) => (writeStr("shellport.db.treeWidth", String(w)), w)),
              );
            }}
            onDoubleClick={() => setTreeW(256)}
          />
        </>
      )}

      {/* Editor + results */}
      <div ref={mainRef} className="flex min-w-0 flex-1 flex-col">
        <div
          className="flex h-10 shrink-0 items-center gap-2 border-b border-border px-2"
          style={profile.color ? { boxShadow: `inset 0 2px 0 ${profile.color}` } : undefined}
        >
          {!treeOpen && (
            <IconButton title="Show schema" onClick={() => setTreeOpen(true)}>
              <PanelLeftOpen size={14} />
            </IconButton>
          )}
          <Button size="sm" variant="primary" onClick={() => run()} disabled={!conn || running} title="Run (Ctrl+Enter). Runs the selection if there is one.">
            <Play size={13} /> Run
          </Button>
          <Button size="sm" onClick={cancel} disabled={!running || !conn} title="Cancel the running query">
            <Square size={12} /> Cancel
          </Button>
          <label className="flex shrink-0 items-center gap-1.5 whitespace-nowrap text-xs text-fg-muted">
            Row limit
            <Select className="h-7 w-24 px-2 text-xs" value={rowLimit} onChange={(e) => setRowLimit(Number(e.target.value))}>
              {ROW_LIMITS.map((n) => (
                <option key={n} value={n}>
                  {n.toLocaleString()}
                </option>
              ))}
            </Select>
          </label>
          {tx !== "idle" && conn && (
            <div
              className={clsx(
                "flex shrink-0 items-center gap-1 rounded-md border py-0.5 pl-2 pr-0.5 text-xs",
                tx === "failed" ? "border-danger/40 bg-danger/10 text-danger" : "border-warn/40 bg-warn/10 text-warn",
              )}
              role="status"
            >
              {tx === "failed" ? "Transaction failed: roll back to continue" : "Transaction open"}
              {tx === "open" && (
                <Button size="sm" variant="ghost" className="h-6 px-2 text-xs" onClick={() => endTx("COMMIT")}>
                  Commit
                </Button>
              )}
              <Button size="sm" variant="ghost" className="h-6 px-2 text-xs" onClick={() => endTx("ROLLBACK")}>
                Rollback
              </Button>
            </div>
          )}
          <div className="flex-1" />
          {conn?.production && <ProdBadge />}
          {conn?.safety === "read-only" && (
            <span className="flex shrink-0 items-center gap-1 rounded bg-surface-3 px-1.5 py-0.5 text-[11px] text-fg-muted" title="This connection refuses writes">
              <Lock size={11} /> Read-only
            </span>
          )}
          <span className={clsx("flex min-w-0 items-center gap-1.5 truncate text-xs", conn ? "text-fg-subtle" : "text-fg-muted")}>
            <span className={clsx("h-2 w-2 shrink-0 rounded-full", conn ? "bg-ok" : connecting ? "bg-warn" : "bg-fg-subtle")} />
            <span className="truncate">{conn ? conn.serverVersion : connecting ? "Connecting…" : "Not connected"}</span>
          </span>
        </div>

        {connErr && !conn ? (
          <ConnectionError
            name={profile.name}
            message={connErr}
            retrying={connecting}
            onRetry={connect}
            onEdit={() => setDbEditor(profile)}
            onUnlock={/unlock the vault/i.test(connErr) ? () => setUnlockPrompt(true) : undefined}
          />
        ) : (
          <>
            {/* Editor with line numbers; no wrapping, like a code editor. */}
            <div className="flex shrink-0 bg-surface-0" style={{ height: editorH }}>
              <Gutter lines={lineCount} editorRef={editorRef} />
              <textarea
                ref={editorRef}
                value={sql}
                wrap="off"
                onChange={(e) => setSql(e.target.value)}
                onScroll={(e) => {
                  const g = e.currentTarget.previousElementSibling as HTMLElement | null;
                  if (g) g.scrollTop = e.currentTarget.scrollTop;
                }}
                onKeyDown={(e) => {
                  if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
                    e.preventDefault();
                    run();
                  }
                  if (e.key === "Tab" && !e.shiftKey) {
                    e.preventDefault();
                    const el = e.currentTarget;
                    const { selectionStart: a, selectionEnd: b } = el;
                    setSql(sql.slice(0, a) + "  " + sql.slice(b));
                    requestAnimationFrame(() => el.setSelectionRange(a + 2, a + 2));
                  }
                }}
                spellCheck={false}
                aria-label="SQL editor"
                placeholder="Write SQL. Ctrl+Enter runs it (or just the selection)."
                className="min-w-0 flex-1 resize-none overflow-auto whitespace-pre bg-surface-0 py-3 pl-2 pr-3 font-mono text-sm leading-5 text-fg outline-none placeholder:text-fg-subtle"
              />
            </div>
            <div
              role="separator"
              aria-orientation="horizontal"
              title="Drag to resize"
              className="h-1.5 shrink-0 cursor-row-resize border-y border-border bg-surface-1 hover:bg-accent/40"
              onMouseDown={(e) => {
                const h0 = editorH;
                const max = (mainRef.current?.clientHeight ?? 600) - 140;
                startDrag(
                  e,
                  "row-resize",
                  (_, dy) => setEditorH(clamp(h0 + dy, 64, Math.max(64, max))),
                  () => setEditorH((h) => (writeStr("shellport.db.editorHeight", String(h)), h)),
                );
              }}
              onDoubleClick={() => setEditorH(200)}
            />
            <div className="flex min-h-0 flex-1 flex-col">
              {output && output.results.length > 1 && (
                <div className="flex shrink-0 gap-0.5 overflow-x-auto border-b border-border px-1 pt-1">
                  {output.results.map((r, i) => (
                    <button
                      key={i}
                      onClick={() => setResultIdx(i)}
                      className={clsx("rounded-t-md px-2.5 py-1 text-xs cursor-pointer", i === resultIdx ? "bg-surface-3 text-fg" : "text-fg-muted hover:bg-surface-2")}
                    >
                      {i + 1}. {r.columns.length ? `${r.rows.length}${r.truncated ? "+" : ""} rows` : `${r.rowsAffected ?? 0} affected`}
                    </button>
                  ))}
                </div>
              )}
              {lostNotice && (
                <div className="flex shrink-0 items-center gap-2 border-b border-warn/30 bg-warn/10 px-3 py-1.5 text-xs text-warn" role="status">
                  <RefreshCw size={12} className={clsx(connecting && "animate-spin")} />
                  <span className="flex-1">
                    {connecting ? "Connection lost. Reconnecting…" : "The connection dropped and was re-established. Your last query was not run again."}
                  </span>
                  {!connecting && conn && (
                    <Button size="sm" variant="ghost" className="h-6 px-2 text-xs" onClick={() => run(lostNotice)}>
                      Run again
                    </Button>
                  )}
                  <IconButton title="Dismiss" className="h-6 w-6" onClick={() => setLostNotice(null)}>
                    <X size={12} />
                  </IconButton>
                </div>
              )}
              <div ref={resultsRef} className="min-h-0 flex-1 overflow-auto">
                {running && !output && (
                  <div className="flex h-full items-center justify-center gap-2 text-sm text-fg-subtle">
                    <Loader2 size={14} className="animate-spin" /> Running…
                  </div>
                )}
                {queryErr && <pre className="m-3 whitespace-pre-wrap break-words rounded-md border border-danger/30 bg-danger/10 p-3 font-mono text-xs text-danger">{queryErr}</pre>}
                {current && <ResultGrid key={`${resultIdx}:${output?.elapsedMs}`} r={current} scrollRef={resultsRef} />}
                {!output && !queryErr && !running && conn && (
                  <Empty icon={<Play size={28} />} title="Run a query">
                    <span className="block">
                      <Kbd>Ctrl</Kbd>+<Kbd>Enter</Kbd> runs the editor, or just the selected text.
                    </span>
                    <span className="mt-1 block">Double-click a table to preview it; right-click for more.</span>
                  </Empty>
                )}
              </div>
              {output && <StatusBar out={output} r={current} running={running} />}
            </div>
          </>
        )}
      </div>

      {confirm && (
        <Modal
          title="Run risky statements?"
          onClose={() => setConfirm(null)}
          width="max-w-2xl"
          footer={
            <>
              <span className="mr-auto text-xs text-fg-subtle">Safety level: Confirm risky. Change it in the connection settings.</span>
              <Button onClick={() => setConfirm(null)}>Cancel</Button>
              <Button
                variant="danger"
                autoFocus
                onClick={() => {
                  const c = confirm;
                  setConfirm(null);
                  run(c.sql, true);
                }}
              >
                Run anyway
              </Button>
            </>
          }
        >
          <p className="mb-3 text-sm text-fg-muted">
            On <span className="font-medium text-fg">{profile.name}</span>:
          </p>
          <ul className="space-y-2">
            {confirm.risks.map((r, i) => (
              <li key={i} className="rounded-md border border-danger/30 bg-danger/5 p-3">
                <div className="flex items-center gap-1.5 text-sm font-medium text-danger">
                  <ShieldAlert size={14} /> {r.reason}
                </div>
                <pre className="mt-1.5 whitespace-pre-wrap break-words font-mono text-xs text-fg-muted">{r.statement}</pre>
              </li>
            ))}
          </ul>
        </Modal>
      )}

      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          onClose={() => setMenu(null)}
          items={[
            ...(menu.node.previewSql ? [{ label: "Preview rows", action: () => preview(menu.node) }] : []),
            { label: "Insert into editor", action: () => insert(menu.node.insertText || menu.node.name) },
            {
              label: "Copy name",
              action: () => navigator.clipboard.writeText(menu.node.insertText || menu.node.name).then(() => toast("Name copied")),
            },
            ...(menu.node.previewSql
              ? [{ label: "Copy SELECT query", action: () => navigator.clipboard.writeText(menu.node.previewSql!).then(() => toast("Query copied")) }]
              : []),
          ]}
        />
      )}
    </div>
  );
}

function plural(n: number, word: string) {
  return `${n.toLocaleString()} ${word}${n === 1 ? "" : "s"}`;
}

function Kbd({ children }: { children: React.ReactNode }) {
  return <kbd className="mx-0.5 rounded border border-border bg-surface-2 px-1.5 py-0.5 font-mono text-[11px] text-fg-muted">{children}</kbd>;
}

function Gutter({ lines, editorRef }: { lines: number; editorRef: React.RefObject<HTMLTextAreaElement | null> }) {
  return (
    <div
      aria-hidden
      className="w-10 shrink-0 select-none overflow-hidden border-r border-border/60 bg-surface-1 py-3 pr-2 text-right font-mono text-xs leading-5 text-fg-subtle"
      onMouseDown={(e) => {
        e.preventDefault();
        editorRef.current?.focus();
      }}
    >
      {Array.from({ length: lines }, (_, i) => (
        <div key={i}>{i + 1}</div>
      ))}
      {/* Room for the textarea's horizontal scrollbar, so numbers stay aligned at the bottom. */}
      <div className="h-6" />
    </div>
  );
}

function ConnectionError({
  name,
  message,
  retrying,
  onRetry,
  onEdit,
  onUnlock,
}: {
  name: string;
  message: string;
  retrying: boolean;
  onRetry: () => void;
  onEdit: () => void;
  onUnlock?: () => void;
}) {
  return (
    <div className="flex min-h-0 flex-1 items-center justify-center overflow-auto p-8">
      <div className="w-full max-w-lg rounded-xl border border-border bg-surface-1 p-6">
        <div className="flex items-center gap-3">
          <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-danger/15 text-danger">{onUnlock ? <Lock size={18} /> : <Unplug size={18} />}</div>
          <div className="min-w-0">
            <div className="text-sm font-semibold">{onUnlock ? "The vault is locked" : "Couldn't connect"}</div>
            <div className="truncate text-xs text-fg-muted">{name}</div>
          </div>
        </div>
        <pre className="mt-4 max-h-48 overflow-auto whitespace-pre-wrap break-words rounded-md bg-surface-0 p-3 font-mono text-xs text-fg-muted">{message}</pre>
        <div className="mt-4 flex justify-end gap-2">
          <Button size="sm" variant="ghost" onClick={onEdit}>
            <Pencil size={13} /> Edit connection
          </Button>
          {onUnlock ? (
            <Button size="sm" variant="primary" onClick={onUnlock}>
              <Lock size={13} /> Unlock vault
            </Button>
          ) : (
            <Button size="sm" variant="primary" onClick={onRetry} disabled={retrying}>
              <RefreshCw size={13} className={clsx(retrying && "animate-spin")} /> {retrying ? "Connecting…" : "Retry"}
            </Button>
          )}
        </div>
      </div>
    </div>
  );
}

function ContextMenu({ x, y, items, onClose }: { x: number; y: number; items: { label: string; action: () => void }[]; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x, y });
  useEffect(() => {
    // Keep the menu inside the window.
    const r = ref.current?.getBoundingClientRect();
    if (r) setPos({ x: Math.min(x, window.innerWidth - r.width - 8), y: Math.min(y, window.innerHeight - r.height - 8) });
    ref.current?.querySelector("button")?.focus();
    const close = (e: Event) => {
      if (e instanceof KeyboardEvent && e.key !== "Escape") return;
      if (e instanceof MouseEvent && ref.current?.contains(e.target as Node)) return;
      onClose();
    };
    window.addEventListener("mousedown", close, true);
    window.addEventListener("keydown", close, true);
    window.addEventListener("blur", onClose);
    return () => {
      window.removeEventListener("mousedown", close, true);
      window.removeEventListener("keydown", close, true);
      window.removeEventListener("blur", onClose);
    };
  }, [x, y, onClose]);
  return (
    <div ref={ref} role="menu" className="fixed z-50 min-w-44 rounded-lg border border-border bg-surface-2 p-1 shadow-2xl" style={{ left: pos.x, top: pos.y }}>
      {items.map((it) => (
        <button
          key={it.label}
          role="menuitem"
          className="flex w-full items-center rounded-md px-2.5 py-1.5 text-left text-xs text-fg hover:bg-surface-4 focus:bg-surface-4 focus:outline-none cursor-pointer"
          onClick={() => {
            onClose();
            it.action();
          }}
          onKeyDown={(e) => {
            const sib = e.key === "ArrowDown" ? e.currentTarget.nextElementSibling : e.key === "ArrowUp" ? e.currentTarget.previousElementSibling : null;
            if (sib instanceof HTMLElement) {
              e.preventDefault();
              sib.focus();
            }
          }}
        >
          {it.label}
        </button>
      ))}
    </div>
  );
}

function StatusBar({ out, r, running }: { out: QueryOutput; r?: StatementResult; running: boolean }) {
  const toast = useApp((s) => s.toast);
  const copyAll = () => {
    if (!r) return;
    const lines = [r.columns.map((c) => c.name).join("\t"), ...r.rows.map((row) => row.map(cellText).join("\t"))];
    navigator.clipboard.writeText(lines.join("\n")).then(() => toast(`Copied ${r.rows.length} rows`));
  };
  return (
    <div className="flex h-7 shrink-0 items-center gap-3 border-t border-border bg-surface-1 px-3 text-xs text-fg-muted">
      {running && <Loader2 size={12} className="animate-spin" />}
      {r && (r.columns.length ? <span>{plural(r.rows.length, "row")}</span> : <span>{plural(r.rowsAffected ?? 0, "row")} affected</span>)}
      {r?.truncated && <span className="text-warn">Stopped at the row limit; more rows exist</span>}
      <span>{out.elapsedMs} ms</span>
      {out.notices.length > 0 && (
        <span className="truncate text-accent" title={out.notices.join("\n")}>
          {out.notices[out.notices.length - 1]}
          {out.notices.length > 1 ? ` (+${out.notices.length - 1})` : ""}
        </span>
      )}
      <div className="flex-1" />
      {r && r.columns.length > 0 && (
        <button className="flex items-center gap-1 rounded px-1.5 py-0.5 hover:bg-surface-3 hover:text-fg cursor-pointer" onClick={copyAll} title="Copy as tab-separated text">
          <Copy size={12} /> Copy TSV
        </button>
      )}
    </div>
  );
}

// ---------- result grid ----------

function cellText(c: Cell): string {
  switch (c.t) {
    case "null":
      return "";
    case "bool":
      return c.v ? "true" : "false";
    default:
      return String(c.v);
  }
}

function CellView({ c }: { c: Cell }) {
  switch (c.t) {
    case "null":
      return <span className="italic text-fg-subtle">NULL</span>;
    case "bool":
      return <span className={c.v ? "text-ok" : "text-fg-muted"}>{c.v ? "true" : "false"}</span>;
    case "json":
      return <span className="text-accent">{c.v}</span>;
    default:
      return <>{String(c.v)}</>;
  }
}

/** Type names whose text values are numbers (e.g. Postgres numeric/money). */
const NUMERIC_TYPE = /^(numeric|decimal|money|int\d?|float\d?|oid)$/;

function compareCells(a: Cell, b: Cell, numeric: boolean): number {
  if (a.t === "null" || b.t === "null") return a.t === b.t ? 0 : a.t === "null" ? 1 : -1;
  if (numeric || (typeof (a as { v: unknown }).v === "number" && typeof (b as { v: unknown }).v === "number")) {
    const x = Number((a as { v: unknown }).v);
    const y = Number((b as { v: unknown }).v);
    if (!Number.isNaN(x) && !Number.isNaN(y)) return x - y;
  }
  return cellText(a).localeCompare(cellText(b), undefined, { numeric: true });
}

/** Text that reads as a number (for results that arrive untyped, e.g. behind a pooler). */
const NUMBER_TEXT = /^-?(\d+\.?\d*|\.\d+)(e[+-]?\d+)?$/i;
/** Rows drawn beyond the visible area on each side. */
const OVERSCAN = 30;

function ResultGrid({ r, scrollRef }: { r: StatementResult; scrollRef: React.RefObject<HTMLDivElement | null> }) {
  const toast = useApp((s) => s.toast);
  const [sel, setSel] = useState<[number, number] | null>(null);
  const [sort, setSort] = useState<{ col: number; dir: 1 | -1 } | null>(null);
  const [widths, setWidths] = useState<Record<number, number>>({});
  const [viewing, setViewing] = useState<{ row: number; col: number } | null>(null);
  const tableRef = useRef<HTMLTableElement>(null);
  const theadRef = useRef<HTMLTableSectionElement>(null);
  // Only the rows in view are drawn; spacers stand in for the rest.
  const [view, setView] = useState({ top: 0, height: 800 });
  const [rowH, setRowH] = useState(22);

  const numeric = useMemo(
    () =>
      r.columns.map((c, i) => {
        if (NUMERIC_TYPE.test(c.typeName) || r.rows.some((row) => row[i]?.t === "int" || row[i]?.t === "float")) return true;
        if (c.typeName) return false;
        const sample = r.rows.slice(0, 200).map((row) => row[i]).filter((x) => x && x.t !== "null");
        return sample.length > 0 && sample.every((x) => x.t === "text" && NUMBER_TEXT.test(x.v));
      }),
    [r],
  );
  /** Starting column widths from the header and a sample of rows, so columns don't jump while scrolling. */
  const autoCh = useMemo(
    () =>
      r.columns.map((c, i) => {
        let w = c.name.length + (c.typeName ? c.typeName.length + 2 : 0) + 3;
        for (const row of r.rows.slice(0, 300)) w = Math.max(w, cellText(row[i]).length + 2);
        return clamp(w, 5, 50);
      }),
    [r],
  );

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const update = () => setView({ top: el.scrollTop, height: el.clientHeight });
    update();
    el.addEventListener("scroll", update, { passive: true });
    const ro = new ResizeObserver(update);
    ro.observe(el);
    return () => {
      el.removeEventListener("scroll", update);
      ro.disconnect();
    };
  }, [scrollRef]);

  // Measure the real row height once rows are on screen.
  useLayoutEffect(() => {
    const tr = tableRef.current?.querySelector<HTMLElement>("tr[data-row]");
    if (tr && tr.offsetHeight && tr.offsetHeight !== rowH) setRowH(tr.offsetHeight);
  });
  /** Display order → row index. */
  const order = useMemo(() => {
    const idx = r.rows.map((_, i) => i);
    if (sort) idx.sort((a, b) => sort.dir * compareCells(r.rows[a][sort.col], r.rows[b][sort.col], numeric[sort.col]));
    return idx;
  }, [r, sort, numeric]);

  // Keep the selected cell in view: rows by scroll position (they may not be
  // drawn yet), columns by the browser once the row is drawn.
  useEffect(() => {
    const el = scrollRef.current;
    if (!sel || !el) return;
    const headH = theadRef.current?.offsetHeight ?? 26;
    const top = sel[0] * rowH;
    if (top < el.scrollTop) el.scrollTop = top;
    else if (top + rowH + headH > el.scrollTop + el.clientHeight) el.scrollTop = top + rowH + headH - el.clientHeight;
    requestAnimationFrame(() => tableRef.current?.querySelector(`[data-cell="${sel[0]}:${sel[1]}"]`)?.scrollIntoView({ block: "nearest", inline: "nearest" }));
  }, [sel, rowH, scrollRef]);

  if (!r.columns.length) return <Empty title="Statement done">{plural(r.rowsAffected ?? 0, "row")} affected</Empty>;

  const cellAt = (dr: number, c: number) => r.rows[order[dr]][c];
  const widthStyle = (i: number) => (widths[i] ? { width: widths[i], minWidth: widths[i], maxWidth: widths[i] } : { minWidth: `${autoCh[i]}ch` });
  const headH = theadRef.current?.offsetHeight ?? 26;
  const first = Math.max(0, Math.floor((view.top - headH) / rowH) - OVERSCAN);
  const last = Math.min(order.length, Math.ceil((view.top + view.height) / rowH) + OVERSCAN);

  return (
    <>
      <table
        ref={tableRef}
        className="select-none border-separate border-spacing-0 font-mono text-xs outline-none"
        tabIndex={0}
        aria-label="Query results"
        onKeyDown={(e) => {
          if (!r.rows.length) return;
          const [row, col] = sel ?? [0, 0];
          const move = (dr: number, dc: number) => {
            e.preventDefault();
            setSel([clamp(row + dr, 0, r.rows.length - 1), clamp(col + dc, 0, r.columns.length - 1)]);
          };
          if (!sel && e.key.startsWith("Arrow")) return move(0, 0);
          if (e.key === "ArrowDown") move(1, 0);
          else if (e.key === "ArrowUp") move(-1, 0);
          else if (e.key === "ArrowRight") move(0, 1);
          else if (e.key === "ArrowLeft") move(0, -1);
          else if (e.key === "PageDown") move(20, 0);
          else if (e.key === "PageUp") move(-20, 0);
          else if (e.key === "Home") move(e.ctrlKey ? -row : 0, -col);
          else if (e.key === "End") move(e.ctrlKey ? r.rows.length : 0, r.columns.length);
          else if (e.key === "Enter" && sel) {
            e.preventDefault();
            setViewing({ row: order[sel[0]], col: sel[1] });
          } else if ((e.ctrlKey || e.metaKey) && e.key === "c" && sel) {
            e.preventDefault();
            navigator.clipboard.writeText(cellText(cellAt(sel[0], sel[1]))).then(() => toast("Cell copied"));
          }
        }}
      >
        <thead ref={theadRef} className="sticky top-0 z-10">
          <tr>
            <th className="border-b border-r border-border bg-surface-2 px-2 py-1 text-right font-normal text-fg-subtle">#</th>
            {r.columns.map((c, i) => (
              <th
                key={i}
                style={widthStyle(i)}
                className="group/th relative cursor-pointer select-none border-b border-r border-border bg-surface-2 px-2 py-1 text-left font-semibold whitespace-nowrap hover:bg-surface-3"
                title={`${c.name}${c.typeName ? ` (${c.typeName})` : ""}. Click to sort`}
                onClick={() => setSort((s) => (s?.col !== i ? { col: i, dir: 1 } : s.dir === 1 ? { col: i, dir: -1 } : null))}
                aria-sort={sort?.col === i ? (sort.dir === 1 ? "ascending" : "descending") : undefined}
              >
                <span className="flex items-center gap-1 overflow-hidden">
                  <span className="truncate">{c.name}</span>
                  {c.typeName && <span className="truncate font-normal text-fg-subtle">{c.typeName}</span>}
                  {sort?.col === i && (sort.dir === 1 ? <ArrowUp size={11} className="shrink-0 text-accent" /> : <ArrowDown size={11} className="shrink-0 text-accent" />)}
                </span>
                <span
                  className="absolute -right-1 top-0 z-10 h-full w-2 cursor-col-resize hover:bg-accent/40"
                  title="Drag to resize; double-click to reset"
                  onClick={(e) => e.stopPropagation()}
                  onDoubleClick={(e) => {
                    e.stopPropagation();
                    setWidths(({ [i]: _, ...rest }) => rest);
                  }}
                  onMouseDown={(e) => {
                    e.stopPropagation();
                    const w0 = (e.currentTarget.parentElement as HTMLElement).getBoundingClientRect().width;
                    startDrag(e, "col-resize", (dx) => setWidths((w) => ({ ...w, [i]: clamp(w0 + dx, 48, 1200) })));
                  }}
                />
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {first > 0 && <tr aria-hidden style={{ height: first * rowH }} />}
          {order.slice(first, last).map((ri, k) => {
            const dr = first + k;
            return (
            <tr key={ri} data-row className="hover:bg-surface-2">
              <td className="border-b border-r border-border px-2 py-0.5 text-right text-fg-subtle">{dr + 1}</td>
              {r.rows[ri].map((c, ci) => (
                <td
                  key={ci}
                  data-cell={`${dr}:${ci}`}
                  style={widthStyle(ci)}
                  onClick={() => setSel([dr, ci])}
                  onDoubleClick={() => setViewing({ row: ri, col: ci })}
                  title={c.t === "null" ? undefined : "Double-click or Enter to view the full value"}
                  className={clsx(
                    "truncate border-b border-r border-border px-2 py-0.5 whitespace-nowrap",
                    !widths[ci] && "max-w-96",
                    numeric[ci] && "text-right",
                    sel?.[0] === dr && sel?.[1] === ci && "bg-accent/20 outline outline-1 -outline-offset-1 outline-accent",
                  )}
                >
                  <CellView c={c} />
                </td>
              ))}
            </tr>
            );
          })}
          {last < order.length && <tr aria-hidden style={{ height: (order.length - last) * rowH }} />}
        </tbody>
      </table>
      {viewing && (
        <ValueViewer
          column={r.columns[viewing.col]}
          cell={r.rows[viewing.row][viewing.col]}
          onClose={() => {
            setViewing(null);
            tableRef.current?.focus();
          }}
        />
      )}
    </>
  );
}

function ValueViewer({ column, cell, onClose }: { column: { name: string; typeName: string }; cell: Cell; onClose: () => void }) {
  const toast = useApp((s) => s.toast);
  let text = cellText(cell);
  if (cell.t === "json") {
    try {
      text = JSON.stringify(JSON.parse(text), null, 2);
    } catch {
      /* not valid JSON: show as is */
    }
  }
  return (
    <Modal
      title={`${column.name}${column.typeName ? ` · ${column.typeName}` : ""}`}
      onClose={onClose}
      width="max-w-2xl"
      footer={
        <>
          <span className="mr-auto text-xs text-fg-subtle">{cell.t === "null" ? "NULL" : `${text.length.toLocaleString()} characters`}</span>
          <Button onClick={() => navigator.clipboard.writeText(text).then(() => toast("Value copied"))} disabled={cell.t === "null"}>
            <Copy size={14} /> Copy
          </Button>
          <Button variant="primary" onClick={onClose}>
            Close
          </Button>
        </>
      }
    >
      {cell.t === "null" ? <div className="text-sm italic text-fg-subtle">NULL</div> : <TextArea readOnly value={text} rows={Math.min(20, Math.max(4, text.split("\n").length))} className="text-xs" />}
    </Modal>
  );
}

// ---------- schema tree ----------

const KIND_ICON: Record<string, React.ReactNode> = {
  schema: <Layers size={13} />,
  folder: <Folder size={13} />,
  table: <Table2 size={13} />,
  view: <Eye size={13} />,
  column: <Columns3 size={13} />,
  function: <FunctionSquare size={13} />,
  procedure: <FunctionSquare size={13} />,
};

/** Kinds the filter applies to; schemas, folders and columns always show. */
const FILTERED_KINDS = new Set(["table", "view", "function", "procedure"]);

const TreeCtx = createContext<{
  sessionId: string;
  filter: string;
  onPreview: (n: SchemaNode) => void;
  onMenu: (e: React.MouseEvent, n: SchemaNode) => void;
}>({ sessionId: "", filter: "", onPreview: () => {}, onMenu: () => {} });

function TreeLevel({ path, depth }: { path: string[]; depth: number }) {
  const { sessionId, filter } = useContext(TreeCtx);
  const [nodes, setNodes] = useState<SchemaNode[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const key = path.join("\u0000");
  useEffect(() => {
    let live = true;
    api
      .dbChildren(sessionId, path)
      .then((n) => live && setNodes(n))
      .catch((e) => live && setErr(errMsg(e)));
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId, key]);
  const pad = { paddingLeft: 22 + depth * 12 };
  if (err) return <div className="px-2 py-1 text-danger" style={pad}>{err}</div>;
  if (!nodes)
    return (
      <div className="flex items-center gap-1.5 py-1 text-fg-subtle" style={pad}>
        <Loader2 size={12} className="animate-spin" /> Loading…
      </div>
    );
  const shown = filter ? nodes.filter((n) => !FILTERED_KINDS.has(n.kind) || n.name.toLowerCase().includes(filter)) : nodes;
  if (!shown.length)
    return (
      <div className="py-1 italic text-fg-subtle" style={pad}>
        {nodes.length ? "no matches" : "empty"}
      </div>
    );
  return (
    <div role="group">
      {shown.map((n) => (
        <TreeNode key={n.path.join("\u0000") + n.name} n={n} depth={depth} />
      ))}
    </div>
  );
}

/** Move focus to the next/previous visible tree row. */
function focusSibling(el: HTMLElement, step: 1 | -1) {
  const all = Array.from(el.closest("[data-tree]")?.querySelectorAll<HTMLElement>("[data-tree-node]") ?? []);
  all[all.indexOf(el) + step]?.focus();
}

function TreeNode({ n, depth }: { n: SchemaNode; depth: number }) {
  const { onPreview, onMenu } = useContext(TreeCtx);
  const [open, setOpen] = useState(false);
  return (
    <>
      <div
        data-tree-node
        role="treeitem"
        aria-expanded={n.hasChildren ? open : undefined}
        tabIndex={0}
        className="mx-1 flex cursor-pointer select-none items-center gap-1 rounded py-0.5 pr-2 outline-none hover:bg-surface-3 focus-visible:bg-surface-3 focus-visible:ring-1 focus-visible:ring-accent"
        style={{ paddingLeft: 4 + depth * 12 }}
        onClick={() => n.hasChildren && setOpen((o) => !o)}
        onDoubleClick={() => onPreview(n)}
        onContextMenu={(e) => onMenu(e, n)}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown") focusSibling(e.currentTarget, 1);
          else if (e.key === "ArrowUp") focusSibling(e.currentTarget, -1);
          else if (e.key === "ArrowRight" && n.hasChildren && !open) setOpen(true);
          else if (e.key === "ArrowRight" && open) focusSibling(e.currentTarget, 1);
          else if (e.key === "ArrowLeft" && open) setOpen(false);
          else if (e.key === "Enter") n.previewSql ? onPreview(n) : n.hasChildren && setOpen((o) => !o);
          else if (e.key === " " && n.hasChildren) setOpen((o) => !o);
          else if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) {
            const r = e.currentTarget.getBoundingClientRect();
            onMenu({ preventDefault: () => {}, clientX: r.left + 24, clientY: r.bottom } as React.MouseEvent, n);
          } else return;
          e.preventDefault();
        }}
        title={n.previewSql ? "Double-click or Enter to preview rows; right-click for more" : undefined}
      >
        <span className="w-3.5 shrink-0 text-fg-subtle">{n.hasChildren ? open ? <ChevronDown size={12} /> : <ChevronRight size={12} /> : null}</span>
        <span className={clsx("shrink-0", n.system ? "text-fg-subtle" : "text-fg-muted")}>{KIND_ICON[n.kind] ?? <Square size={11} />}</span>
        <span className={clsx("shrink-0 truncate", n.system && "text-fg-subtle")} style={{ maxWidth: "70%" }} title={n.name}>
          {n.name}
        </span>
        {(n.detail || n.system) && (
          <span className="ml-auto min-w-0 truncate pl-2 text-right text-[11px] text-fg-subtle" title={n.detail ?? undefined}>
            {n.detail ?? "system"}
          </span>
        )}
      </div>
      {open && <TreeLevel path={n.path} depth={depth + 1} />}
    </>
  );
}

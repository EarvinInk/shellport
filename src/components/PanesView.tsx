import { ask } from "@tauri-apps/plugin-dialog";
import clsx from "clsx";
import { Columns2, Radio, Rows2, X } from "lucide-react";
import { useEffect, useMemo, useRef } from "react";
import { api } from "../lib/ipc";
import { allPanes, commandAppliesTo, fillCommand, layoutRects, leaf, removePane, setRatio, splitPane } from "../lib/panes";
import type { PaneNode, PaneTarget } from "../lib/types";
import { onTabClosed, terminalWriter, useApp, type Tab } from "../store";
import { TerminalView } from "./TerminalView";
import { Button, IconButton } from "./ui";

type TermTab = Extract<Tab, { kind: "local" | "ssh" | "panes" }>;

/** The layout of a terminal tab; plain terminal tabs are a single pane with the tab's id. */
export function tabLayout(t: TermTab): PaneNode {
  if (t.kind === "panes") return t.layout;
  return leaf(t.id, t.kind === "ssh" ? { kind: "ssh", hostId: t.hostId } : { kind: "local" });
}

const focused = new Map<string, string>();
/** The pane last focused in a tab (for palette actions such as split). */
export function focusedPane(tabId: string): string | undefined {
  return focused.get(tabId);
}

/** Split a pane of a terminal tab (turning a plain terminal tab into a panes tab). */
export function splitTab(tabId: string, dir: "row" | "col", target?: PaneTarget, paneId?: string) {
  const st = useApp.getState();
  const t = st.tabs.find((x) => x.id === tabId);
  if (!t || (t.kind !== "local" && t.kind !== "ssh" && t.kind !== "panes")) return;
  const layout = tabLayout(t);
  const panes = allPanes(layout);
  const from = panes.find((p) => p.id === (paneId ?? focused.get(tabId))) ?? panes[panes.length - 1];
  const next = splitPane(layout, from.id, dir, target ?? from.target);
  useApp.setState((s) => ({ tabs: s.tabs.map((x) => (x.id === tabId ? { id: x.id, kind: "panes", title: x.title, layout: next } : x)) }));
}

/** Close one pane; the last one closes the tab. */
export async function closePane(tabId: string, paneId: string) {
  const st = useApp.getState();
  const t = st.tabs.find((x) => x.id === tabId);
  if (!t || (t.kind !== "local" && t.kind !== "ssh" && t.kind !== "panes")) return;
  const layout = tabLayout(t);
  const pane = allPanes(layout).find((p) => p.id === paneId);
  const next = removePane(layout, paneId);
  if (!next) return st.closeTab(tabId);
  // A closed pane ends its tmux session, like closing a tab does.
  if (pane?.target.kind === "ssh") {
    const host = st.hosts.find((h) => h.id === (pane.target as { hostId: string }).hostId);
    if (host?.tmuxSession) api.termKillTmux(host.id, paneId).catch(() => {});
  }
  const group = st.broadcast[tabId];
  if (group) st.setBroadcast(tabId, group.filter((p) => p !== paneId));
  useApp.setState((s) => ({ tabs: s.tabs.map((x) => (x.id === tabId && x.kind === "panes" ? { ...x, layout: next } : x)) }));
}

/** Turn broadcast on for every pane of a tab (asking first if any is on a production host), or off. */
export async function toggleBroadcast(tabId: string) {
  const st = useApp.getState();
  const t = st.tabs.find((x) => x.id === tabId);
  if (!t || (t.kind !== "local" && t.kind !== "ssh" && t.kind !== "panes")) return;
  if (st.broadcast[tabId]) return st.setBroadcast(tabId, null);
  const panes = allPanes(tabLayout(t));
  if (panes.length < 2) return st.toast("Split the tab first: broadcast sends your typing to every pane of a tab.");
  const prod = panes.map((p) => (p.target.kind === "ssh" ? st.hosts.find((h) => h.id === (p.target as { hostId: string }).hostId) : null)).filter((h) => h?.production);
  if (prod.length && !(await ask(`Typing will go to ${panes.length} panes, including production: ${[...new Set(prod.map((h) => h!.name))].join(", ")}.`, { title: "Broadcast to production?", kind: "warning", okLabel: "Broadcast" }))) return;
  st.setBroadcast(tabId, panes.map((p) => p.id));
}

export function PanesView({ tab, active }: { tab: TermTab; active: boolean }) {
  const layout = useMemo(() => tabLayout(tab), [tab]);
  const { panes, splits } = useMemo(() => layoutRects(layout), [layout]);
  const group = useApp((s) => s.broadcast[tab.id]);
  const boxRef = useRef<HTMLDivElement>(null);
  const many = panes.length > 1;

  // Closing the whole tab ends the tmux sessions of all its panes.
  useEffect(() => {
    onTabClosed(tab.id, () => {
      const st = useApp.getState();
      for (const p of allPanes(tabLayout(tab))) {
        if (p.target.kind !== "ssh") continue;
        const host = st.hosts.find((h) => h.id === (p.target as { hostId: string }).hostId);
        if (host?.tmuxSession) api.termKillTmux(host.id, p.id).catch(() => {});
      }
      st.setBroadcast(tab.id, null);
    });
    return () => onTabClosed(tab.id, null);
  }, [tab]);

  const dragSplit = (e: React.MouseEvent, s: (typeof splits)[number]) => {
    e.preventDefault();
    const box = boxRef.current!.getBoundingClientRect();
    const move = (ev: MouseEvent) => {
      const pct = s.dir === "row" ? ((ev.clientX - box.left) / box.width) * 100 : ((ev.clientY - box.top) / box.height) * 100;
      const span = s.dir === "row" ? s.rect.w : s.rect.h;
      const start = s.dir === "row" ? s.rect.x : s.rect.y;
      const ratio = Math.min(0.9, Math.max(0.1, (pct - start) / span));
      useApp.setState((st) => ({ tabs: st.tabs.map((x) => (x.id === tab.id && x.kind === "panes" ? { ...x, layout: setRatio(x.layout, s.path, ratio) } : x)) }));
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      document.body.style.cursor = "";
    };
    document.body.style.cursor = s.dir === "row" ? "col-resize" : "row-resize";
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  return (
    <div className="flex h-full flex-col">
      {group && (
        <div className="flex shrink-0 items-center gap-2 border-b border-warn/40 bg-warn/15 px-3 py-1 text-xs text-warn" role="status">
          <Radio size={13} /> Broadcasting: what you type goes to {group.length} panes.
          <Button size="sm" variant="ghost" className="ml-auto h-6 px-2 text-xs" onClick={() => toggleBroadcast(tab.id)}>
            Stop
          </Button>
        </div>
      )}
      <div ref={boxRef} className="relative min-h-0 flex-1">
        {panes.map(({ id, rect }) => {
          const pane = allPanes(layout).find((p) => p.id === id)!;
          const inGroup = !!group?.includes(id);
          return (
            <div
              key={id}
              className={clsx("group/pane absolute", many && "border border-border", inGroup && "ring-1 ring-inset ring-warn/60")}
              style={{ left: `${rect.x}%`, top: `${rect.y}%`, width: `${rect.w}%`, height: `${rect.h}%` }}
              onFocusCapture={() => focused.set(tab.id, id)}
              onMouseDown={() => focused.set(tab.id, id)}
            >
              <PaneTerminal tab={tab} paneId={id} target={pane.target} active={active} single={!many} />
              <div className="absolute right-3 top-1.5 z-10 hidden gap-0.5 rounded-md border border-border bg-surface-2/95 p-0.5 shadow-lg group-hover/pane:flex">
                <IconButton title="Split right" className="h-6 w-6" onClick={() => splitTab(tab.id, "row", undefined, id)}>
                  <Columns2 size={13} />
                </IconButton>
                <IconButton title="Split down" className="h-6 w-6" onClick={() => splitTab(tab.id, "col", undefined, id)}>
                  <Rows2 size={13} />
                </IconButton>
                {many && (
                  <IconButton
                    title={inGroup ? "Leave broadcast" : group ? "Join broadcast" : "Broadcast typing to all panes"}
                    className={clsx("h-6 w-6", inGroup && "text-warn")}
                    onClick={() => (group ? useApp.getState().setBroadcast(tab.id, inGroup ? group.filter((p) => p !== id) : [...group, id]) : toggleBroadcast(tab.id))}
                  >
                    <Radio size={13} />
                  </IconButton>
                )}
                {many && (
                  <IconButton title="Close pane" className="h-6 w-6" onClick={() => closePane(tab.id, id)}>
                    <X size={13} />
                  </IconButton>
                )}
              </div>
            </div>
          );
        })}
        {splits.map((s) => (
          <div
            key={s.path || "root"}
            role="separator"
            aria-orientation={s.dir === "row" ? "vertical" : "horizontal"}
            title="Drag to resize"
            onMouseDown={(e) => dragSplit(e, s)}
            className={clsx("absolute z-20 hover:bg-accent/40", s.dir === "row" ? "w-1.5 -translate-x-1/2 cursor-col-resize" : "h-1.5 -translate-y-1/2 cursor-row-resize")}
            style={s.dir === "row" ? { left: `${s.at}%`, top: `${s.rect.y}%`, height: `${s.rect.h}%` } : { top: `${s.at}%`, left: `${s.rect.x}%`, width: `${s.rect.w}%` }}
          />
        ))}
      </div>
    </div>
  );
}

/** One terminal inside a tab: local, or SSH to a host with that host's policies. */
function PaneTerminal({ tab, paneId, target, active, single }: { tab: TermTab; paneId: string; target: PaneTarget; active: boolean; single: boolean }) {
  const host = useApp((s) => (target.kind === "ssh" ? s.hosts.find((h) => h.id === target.hostId) : undefined));
  const { renameTab, closeTab, markActivity } = useApp.getState();
  const ranOnConnect = useRef(false);
  const common = {
    tabId: paneId,
    groupTabId: tab.id,
    active,
    onActivity: () => markActivity(tab.id),
  };
  if (target.kind === "local") {
    return <TerminalView {...common} label="Local terminal" onTitle={(x) => single && renameTab(tab.id, x)} open={(c, r, ch) => api.termOpenLocal(c, r, ch)} />;
  }
  return (
    <TerminalView
      {...common}
      label={host?.name}
      production={!!host?.production}
      open={(c, r, ch) => api.termOpenSsh(target.hostId, c, r, ch, paneId)}
      onLost={host?.onDisconnect ?? "reconnect"}
      probeHostId={target.hostId}
      transfers={{ zmodem: !!host?.zmodem }}
      onCloseTab={() => (single ? closeTab(tab.id) : closePane(tab.id, paneId))}
      onOpened={() => {
        // Saved commands marked "run on connect" (M2.2): typed once per new
        // pane after login; with tmux the session already has them.
        if (ranOnConnect.current || host?.tmuxSession) return;
        ranOnConnect.current = true;
        const cmds = useApp.getState().library.commands.filter((c) => c.runOnConnect && commandAppliesTo(c, host));
        if (cmds.length) setTimeout(() => cmds.forEach((c) => terminalWriter(paneId)?.(fillCommand(c.command, {}, host) + "\r")), 400);
      }}
    />
  );
}

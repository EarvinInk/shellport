import type { Host } from "./types";
import { focusedPane } from "../components/PanesView";
import { allPanes } from "./panes";
import { terminalPaster, useApp } from "../store";

/** Terminals (tab id, pane id) connected to `host`, in tab order. */
function terminalsOn(hostId: string): { tabId: string; paneId: string }[] {
  const out: { tabId: string; paneId: string }[] = [];
  for (const t of useApp.getState().tabs) {
    if (t.kind === "ssh" && t.hostId === hostId) out.push({ tabId: t.id, paneId: t.id });
    if (t.kind === "panes") {
      const mine = allPanes(t.layout).filter((p) => p.target.kind === "ssh" && p.target.hostId === hostId);
      // The focused pane first, so it wins within its tab.
      mine.sort((a, b) => Number(b.id === focusedPane(t.id)) - Number(a.id === focusedPane(t.id)));
      out.push(...mine.map((p) => ({ tabId: t.id, paneId: p.id })));
    }
  }
  return out;
}

/**
 * Paste text into a terminal on `host`: the active tab if it is one (its
 * focused pane when split), else the most recent terminal on that host, else
 * a new one (once it has connected). Pasting never presses Enter; with
 * bracketed paste nothing runs until you do.
 */
export async function pasteToHost(host: Host, text: string): Promise<boolean> {
  const { activeTab, openTab, setActive } = useApp.getState();
  const mine = terminalsOn(host.id);
  let target = mine.find((t) => t.tabId === activeTab) ?? mine[mine.length - 1];
  if (target) setActive(target.tabId);
  else {
    const id = openTab({ kind: "ssh", title: host.name, hostId: host.id });
    target = { tabId: id, paneId: id };
  }
  // A new or reconnecting terminal registers its paster once connected.
  for (let i = 0; i < 100; i++) {
    const paste = terminalPaster(target.paneId);
    if (paste) {
      await paste(text);
      return true;
    }
    await new Promise((r) => setTimeout(r, 200));
  }
  return false;
}

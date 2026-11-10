import type { Host } from "./types";
import { terminalPaster, useApp } from "../store";

/**
 * Paste text into a terminal on `host`: the active tab if it is one, else the
 * most recent terminal on that host, else a new one (once it has connected).
 * Pasting never presses Enter; with bracketed paste nothing runs until you do.
 */
export async function pasteToHost(host: Host, text: string): Promise<boolean> {
  const { tabs, activeTab, openTab, setActive } = useApp.getState();
  const mine = tabs.filter((t) => t.kind === "ssh" && t.hostId === host.id);
  let target = mine.find((t) => t.id === activeTab) ?? mine[mine.length - 1];
  if (target) setActive(target.id);
  else target = { id: openTab({ kind: "ssh", title: host.name, hostId: host.id }), kind: "ssh", title: host.name, hostId: host.id };
  // A new or reconnecting terminal registers its paster once connected.
  for (let i = 0; i < 100; i++) {
    const paste = terminalPaster(target.id);
    if (paste) {
      await paste(text);
      return true;
    }
    await new Promise((r) => setTimeout(r, 200));
  }
  return false;
}

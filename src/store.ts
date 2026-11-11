import { create } from "zustand";
import { api, errMsg } from "./lib/ipc";
import type { DbProfile, Host, HostStatus, Library, PaneNode, SecretMeta, Settings, TransferView, VaultStatus } from "./lib/types";

export type Tab =
  | { id: string; kind: "local"; title: string }
  | { id: string; kind: "ssh"; title: string; hostId: string }
  | { id: string; kind: "sftp"; title: string; hostId: string }
  | { id: string; kind: "edit"; title: string; hostId: string; path: string; sudo?: boolean }
  | { id: string; kind: "docker"; title: string; endpoint: string }
  | { id: string; kind: "logs"; title: string; endpoint: string; containerId: string }
  | { id: string; kind: "exec"; title: string; endpoint: string; containerId: string }
  | { id: string; kind: "mlogs"; title: string; endpoint: string; containers: { id: string; name: string }[] }
  | { id: string; kind: "db"; title: string; dbId: string }
  | { id: string; kind: "panes"; title: string; layout: PaneNode }
  | { id: string; kind: "commands"; title: string }
  | { id: string; kind: "activity"; title: string; hostId?: string; dbId?: string }
  | { id: string; kind: "vault"; title: string }
  | { id: string; kind: "settings"; title: string };

export type Sidebar = "hosts" | "databases" | "docker";

type TabInput = Tab extends infer T ? (T extends Tab ? Omit<T, "id"> : never) : never;

export interface Toast {
  id: number;
  kind: "info" | "error" | "success";
  text: string;
}

interface AppStore {
  tabs: Tab[];
  activeTab: string | null;
  hosts: Host[];
  dbs: DbProfile[];
  dbEditor: DbProfile | "new" | null;
  setDbEditor: (d: DbProfile | "new" | null) => void;
  refreshDbs: () => Promise<void>;
  /** Number of connected tabs per database profile id. */
  dbLive: Record<string, number>;
  dbLiveAdd: (id: string, delta: number) => void;
  /** Saved commands and workspaces (M2.2, M2.1). */
  library: Library;
  refreshLibrary: () => Promise<void>;
  /** Live broadcast groups: tab id → pane ids typing goes to (M2.3). */
  broadcast: Record<string, string[]>;
  setBroadcast: (tabId: string, panes: string[] | null) => void;
  /** Background tabs with new output since last viewed (M2.4). */
  tabActivity: Record<string, boolean>;
  markActivity: (tabId: string) => void;
  /** The "run on hosts" dialog (M2.3). */
  /** The app-wide transfer queue (kept in step with the backend's events). */
  transfers: TransferView[];
  setTransfers: (t: TransferView[]) => void;
  upsertTransfer: (t: TransferView) => void;
  transfersOpen: boolean;
  setTransfersOpen: (o: boolean) => void;
  /** Ask an SFTP tab to show a folder (e.g. a favourite picked in the palette). */
  sftpGoto: { hostId: string; path: string; n: number } | null;
  gotoSftp: (hostId: string, path: string) => void;
  runDialog: { hostIds?: string[]; command?: string } | null;
  setRunDialog: (d: { hostIds?: string[]; command?: string } | null) => void;
  /** Replace a panes tab's layout (split, close, resize). */
  setLayout: (tabId: string, layout: PaneNode) => void;
  /** Reachability per host id (M1.5). */
  hostStatus: Record<string, HostStatus["up"]>;
  refreshHostStatus: () => Promise<void>;
  paletteOpen: boolean;
  setPaletteOpen: (o: boolean) => void;
  /** Host whose notes are open. */
  notesHost: string | null;
  setNotesHost: (id: string | null) => void;
  /** Number of tabs with an open transaction per database profile id. */
  dbTx: Record<string, number>;
  dbTxAdd: (id: string, delta: number) => void;
  secrets: SecretMeta[];
  vault: VaultStatus;
  settings: Settings | null;
  toasts: Toast[];
  sidebar: Sidebar;
  sidebarCollapsed: boolean;
  toggleSidebar: () => void;
  hostEditor: Host | "new" | null;
  importOpen: boolean;
  unlockPrompt: boolean;
  forwards: { hostId: string; forwardId: string }[];
  forwardsHost: string | null;
  setForwardsHost: (id: string | null) => void;
  refreshForwards: () => Promise<void>;
  sessionReady: boolean;
  restoreSession: () => Promise<void>;

  openTab: (t: TabInput, opts?: { unique?: boolean }) => string;
  closeTab: (id: string) => void;
  setActive: (id: string) => void;
  renameTab: (id: string, title: string) => void;
  setSidebar: (s: Sidebar) => void;
  setHostEditor: (h: Host | "new" | null) => void;
  setImportOpen: (o: boolean) => void;
  setUnlockPrompt: (o: boolean) => void;

  refreshHosts: () => Promise<void>;
  refreshVault: () => Promise<void>;
  refreshSecrets: () => Promise<void>;
  loadSettings: () => Promise<void>;
  saveSettings: (s: Settings) => Promise<void>;
  toast: (text: string, kind?: Toast["kind"]) => void;
  error: (e: unknown) => void;
  dismissToast: (id: number) => void;
}

let toastSeq = 1;

function bump(m: Record<string, number>, id: string, delta: number): Record<string, number> {
  const n = Math.max(0, (m[id] ?? 0) + delta);
  const next = { ...m };
  if (n) next[id] = n;
  else delete next[id];
  return next;
}

/**
 * A tab can veto its own closing (e.g. an open database transaction). The
 * guard asks the user and resolves true to go ahead.
 */
const closeGuards = new Map<string, () => Promise<boolean>>();
export function setCloseGuard(tabId: string, guard: (() => Promise<boolean>) | null) {
  if (guard) closeGuards.set(tabId, guard);
  else closeGuards.delete(tabId);
}

/** Host ids used by a pane layout. */
export function paneHosts(n: PaneNode): string[] {
  if (n.t === "pane") return n.target.kind === "ssh" ? [n.target.hostId] : [];
  return [...paneHosts(n.a), ...paneHosts(n.b)];
}

/** Raw writers into terminal sessions, by pane/tab id (for broadcast input). */
type Writer = (data: string) => void;
const terminalWriters = new Map<string, Writer>();
export function setTerminalWriter(id: string, w: Writer | null) {
  if (w) terminalWriters.set(id, w);
  else terminalWriters.delete(id);
}
export function terminalWriter(id: string): Writer | undefined {
  return terminalWriters.get(id);
}

/** Plain text of a terminal's scrollback and screen, by pane/tab id. */
const terminalReaders = new Map<string, () => string>();
export function setTerminalReader(id: string, r: (() => string) | null) {
  if (r) terminalReaders.set(id, r);
  else terminalReaders.delete(id);
}
export function terminalReader(id: string): (() => string) | undefined {
  return terminalReaders.get(id);
}

/** Paste into a terminal tab as if the user pasted (respects bracketed paste,
 * so nothing runs until Enter). Lets notes and the palette hand text over. */
type Paster = (text: string) => void | Promise<void>;
const terminalPasters = new Map<string, Paster>();
export function setTerminalPaster(tabId: string, paste: Paster | null) {
  if (paste) terminalPasters.set(tabId, paste);
  else terminalPasters.delete(tabId);
}
export function terminalPaster(tabId: string): Paster | undefined {
  return terminalPasters.get(tabId);
}

/** Runs when the user closes a tab (not when it unmounts for other reasons). */
const tabCloseHooks = new Map<string, () => void>();
export function onTabClosed(tabId: string, hook: (() => void) | null) {
  if (hook) tabCloseHooks.set(tabId, hook);
  else tabCloseHooks.delete(tabId);
}
const uid = () => Math.random().toString(36).slice(2, 10);

export const useApp = create<AppStore>((set, get) => ({
  tabs: [],
  activeTab: null,
  hosts: [],
  dbs: [],
  dbEditor: null,
  dbLive: {},
  dbLiveAdd: (id, delta) => set((s) => ({ dbLive: bump(s.dbLive, id, delta) })),
  dbTx: {},
  library: { commands: [], workspaces: [] },
  refreshLibrary: async () => {
    try {
      set({ library: await api.libraryGet() });
    } catch (e) {
      get().error(e);
    }
  },
  broadcast: {},
  setBroadcast: (tabId, panes) =>
    set((s) => {
      const broadcast = { ...s.broadcast };
      if (panes && panes.length > 1) broadcast[tabId] = panes;
      else delete broadcast[tabId];
      return { broadcast };
    }),
  tabActivity: {},
  markActivity: (tabId) => {
    if (get().activeTab !== tabId && !get().tabActivity[tabId]) set((s) => ({ tabActivity: { ...s.tabActivity, [tabId]: true } }));
  },
  transfers: [],
  setTransfers: (transfers) => set({ transfers }),
  upsertTransfer: (t) =>
    set((s) => {
      const i = s.transfers.findIndex((x) => x.id === t.id);
      if (i < 0) return { transfers: [...s.transfers, t] };
      const next = s.transfers.slice();
      next[i] = t;
      return { transfers: next };
    }),
  transfersOpen: false,
  setTransfersOpen: (transfersOpen) => set({ transfersOpen }),
  sftpGoto: null,
  gotoSftp: (hostId, path) => {
    const h = get().hosts.find((x) => x.id === hostId);
    get().openTab({ kind: "sftp", title: `${h?.name ?? "Host"} · files`, hostId }, { unique: true });
    set((s) => ({ sftpGoto: { hostId, path, n: (s.sftpGoto?.n ?? 0) + 1 } }));
  },
  runDialog: null,
  setRunDialog: (runDialog) => set({ runDialog }),
  setLayout: (tabId, layout) => set((s) => ({ tabs: s.tabs.map((t) => (t.id === tabId && t.kind === "panes" ? { ...t, layout } : t)) })),
  hostStatus: {},
  refreshHostStatus: async () => {
    try {
      const list = await api.hostsStatus();
      set({ hostStatus: Object.fromEntries(list.map((h) => [h.id, h.up])) });
    } catch {
      /* not critical */
    }
  },
  paletteOpen: false,
  setPaletteOpen: (paletteOpen) => set({ paletteOpen }),
  notesHost: null,
  setNotesHost: (notesHost) => set({ notesHost }),
  dbTxAdd: (id, delta) => set((s) => ({ dbTx: bump(s.dbTx, id, delta) })),
  setDbEditor: (dbEditor) => set({ dbEditor }),
  refreshDbs: async () => {
    try {
      set({ dbs: await api.dbsList() });
    } catch (e) {
      get().error(e);
    }
  },
  secrets: [],
  vault: { exists: false, unlocked: false, remembered: false },
  settings: null,
  toasts: [],
  sidebar: "hosts",
  sidebarCollapsed: readCollapsed(),
  toggleSidebar: () =>
    set((s) => {
      const sidebarCollapsed = !s.sidebarCollapsed;
      try {
        localStorage.setItem("shellport.sidebarCollapsed", sidebarCollapsed ? "1" : "0");
      } catch {
        /* storage unavailable: keep in memory only */
      }
      return { sidebarCollapsed };
    }),
  hostEditor: null,
  importOpen: false,
  unlockPrompt: false,
  forwards: [],
  forwardsHost: null,
  sessionReady: false,
  restoreSession: async () => {
    try {
      const st = get();
      if (st.settings?.restoreTabs === false) return;
      const sess = (await api.sessionLoad()) as { tabs?: Tab[]; activeTab?: string | null } | null;
      if (!sess?.tabs?.length) return;
      const hostIds = new Set(st.hosts.map((h) => h.id));
      const dbIds = new Set(st.dbs.map((d) => d.id));
      const tabs = sess.tabs.filter((t) => {
        if (t.kind === "activity") return true;
        if (t.kind === "panes") return paneHosts(t.layout).every((h) => hostIds.has(h));
        if ("hostId" in t) return hostIds.has(t.hostId);
        if ("dbId" in t) return dbIds.has(t.dbId);
        if ("endpoint" in t) return t.endpoint === "local" || hostIds.has(t.endpoint);
        return true;
      });
      const activeTab = tabs.some((t) => t.id === sess.activeTab) ? sess.activeTab! : (tabs[0]?.id ?? null);
      set({ tabs: [...tabs, ...st.tabs], activeTab: st.activeTab ?? activeTab });
    } catch {
      /* corrupt session file: start fresh */
    } finally {
      set({ sessionReady: true });
    }
  },
  setForwardsHost: (forwardsHost) => set({ forwardsHost }),
  refreshForwards: async () => {
    try {
      set({ forwards: await api.forwardsActive() });
    } catch {
      /* ignore */
    }
  },

  openTab: (t, opts) => {
    if (opts?.unique) {
      const existing = get().tabs.find((x) => JSON.stringify({ ...x, id: "", title: "" }) === JSON.stringify({ ...t, id: "", title: "" }));
      if (existing) {
        set({ activeTab: existing.id });
        return existing.id;
      }
    }
    const id = uid();
    set((s) => ({ tabs: [...s.tabs, { ...t, id } as Tab], activeTab: id }));
    return id;
  },
  closeTab: (id) => {
    const guard = closeGuards.get(id);
    if (guard) {
      guard().then((ok) => {
        if (ok) {
          closeGuards.delete(id);
          get().closeTab(id);
        }
      });
      return;
    }
    tabCloseHooks.get(id)?.();
    tabCloseHooks.delete(id);
    try {
      localStorage.removeItem(`shellport.dbsql.tab.${id}`);
    } catch {
      /* storage unavailable */
    }
    set((s) => {
      const idx = s.tabs.findIndex((t) => t.id === id);
      const tabs = s.tabs.filter((t) => t.id !== id);
      let activeTab = s.activeTab;
      if (activeTab === id) activeTab = tabs[Math.min(idx, tabs.length - 1)]?.id ?? null;
      return { tabs, activeTab };
    });
  },
  setActive: (id) =>
    set((s) => {
      if (!s.tabActivity[id]) return { activeTab: id };
      const tabActivity = { ...s.tabActivity };
      delete tabActivity[id];
      return { activeTab: id, tabActivity };
    }),
  renameTab: (id, title) => set((s) => ({ tabs: s.tabs.map((t) => (t.id === id ? { ...t, title } : t)) })),
  setSidebar: (sidebar) => set({ sidebar, sidebarCollapsed: false }),
  setHostEditor: (hostEditor) => set({ hostEditor }),
  setImportOpen: (importOpen) => set({ importOpen }),
  setUnlockPrompt: (unlockPrompt) => set({ unlockPrompt }),

  refreshHosts: async () => {
    try {
      set({ hosts: await api.hostsList() });
    } catch (e) {
      get().error(e);
    }
  },
  refreshVault: async () => {
    const vault = await api.vaultStatus();
    set({ vault });
    if (vault.unlocked) await get().refreshSecrets();
    else set({ secrets: [] });
  },
  refreshSecrets: async () => {
    try {
      set({ secrets: await api.secretsList() });
    } catch {
      set({ secrets: [] });
    }
  },
  loadSettings: async () => set({ settings: await api.settingsGet() }),
  saveSettings: async (s) => {
    try {
      set({ settings: await api.settingsSave(s) });
      get().toast("Settings saved", "success");
    } catch (e) {
      get().error(e);
    }
  },
  toast: (text, kind = "info") => {
    const id = toastSeq++;
    set((s) => ({ toasts: [...s.toasts, { id, kind, text }] }));
    setTimeout(() => get().dismissToast(id), kind === "error" ? 8000 : 3500);
  },
  error: (e) => {
    const msg = errMsg(e);
    get().toast(msg, "error");
    if (/unlock the vault/i.test(msg)) set({ unlockPrompt: true });
  },
  dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
}));

function readCollapsed(): boolean {
  try {
    return localStorage.getItem("shellport.sidebarCollapsed") === "1";
  } catch {
    return false;
  }
}

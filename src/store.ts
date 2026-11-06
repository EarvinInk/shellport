import { create } from "zustand";
import { api, errMsg } from "./lib/ipc";
import type { Host, SecretMeta, Settings, VaultStatus } from "./lib/types";

export type Tab =
  | { id: string; kind: "local"; title: string }
  | { id: string; kind: "ssh"; title: string; hostId: string }
  | { id: string; kind: "sftp"; title: string; hostId: string }
  | { id: string; kind: "docker"; title: string; endpoint: string }
  | { id: string; kind: "logs"; title: string; endpoint: string; containerId: string }
  | { id: string; kind: "exec"; title: string; endpoint: string; containerId: string }
  | { id: string; kind: "mlogs"; title: string; endpoint: string; containers: { id: string; name: string }[] }
  | { id: string; kind: "vault"; title: string }
  | { id: string; kind: "settings"; title: string };

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
  secrets: SecretMeta[];
  vault: VaultStatus;
  settings: Settings | null;
  toasts: Toast[];
  sidebar: "hosts" | "docker";
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
  setSidebar: (s: "hosts" | "docker") => void;
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
const uid = () => Math.random().toString(36).slice(2, 10);

export const useApp = create<AppStore>((set, get) => ({
  tabs: [],
  activeTab: null,
  hosts: [],
  secrets: [],
  vault: { exists: false, unlocked: false, remembered: false },
  settings: null,
  toasts: [],
  sidebar: "hosts",
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
      const tabs = sess.tabs.filter((t) => {
        if ("hostId" in t) return hostIds.has(t.hostId);
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
  closeTab: (id) =>
    set((s) => {
      const idx = s.tabs.findIndex((t) => t.id === id);
      const tabs = s.tabs.filter((t) => t.id !== id);
      let activeTab = s.activeTab;
      if (activeTab === id) activeTab = tabs[Math.min(idx, tabs.length - 1)]?.id ?? null;
      return { tabs, activeTab };
    }),
  setActive: (id) => set({ activeTab: id }),
  renameTab: (id, title) => set((s) => ({ tabs: s.tabs.map((t) => (t.id === id ? { ...t, title } : t)) })),
  setSidebar: (sidebar) => set({ sidebar }),
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

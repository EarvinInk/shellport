import { listen } from "@tauri-apps/api/event";
import clsx from "clsx";
import { Box, FolderOpen, Lock, LockOpen, Plus, Server, Settings as SettingsIcon, ShieldCheck, SquareTerminal, Terminal, X } from "lucide-react";
import { useEffect } from "react";
import { HostKeyDialog, Toasts, UnlockDialog } from "./components/Dialogs";
import { HostEditor } from "./components/HostEditor";
import { DockerSidebar, HostsSidebar } from "./components/HostsSidebar";
import { ImportDialog } from "./components/ImportDialog";
import { TerminalView } from "./components/TerminalView";
import { Empty } from "./components/ui";
import { api } from "./lib/ipc";
import { useApp, type Tab } from "./store";
import { DockerView, ExecView, LogsView } from "./views/DockerView";
import { SettingsView } from "./views/SettingsView";
import { SftpView } from "./views/SftpView";
import { VaultView } from "./views/VaultView";

function TabIcon({ t }: { t: Tab }) {
  const s = 13;
  switch (t.kind) {
    case "local":
      return <Terminal size={s} />;
    case "ssh":
      return <SquareTerminal size={s} />;
    case "sftp":
      return <FolderOpen size={s} />;
    case "docker":
    case "logs":
    case "exec":
      return <Box size={s} />;
    case "vault":
      return <ShieldCheck size={s} />;
    case "settings":
      return <SettingsIcon size={s} />;
  }
}

function TabContent({ t, active }: { t: Tab; active: boolean }) {
  const renameTab = useApp((s) => s.renameTab);
  switch (t.kind) {
    case "local":
      return <TerminalView active={active} onTitle={(x) => renameTab(t.id, x)} open={(c, r, ch) => api.termOpenLocal(c, r, ch)} />;
    case "ssh":
      return <TerminalView active={active} open={(c, r, ch) => api.termOpenSsh(t.hostId, c, r, ch)} />;
    case "sftp":
      return <SftpView hostId={t.hostId} active={active} />;
    case "docker":
      return <DockerView endpoint={t.endpoint} active={active} />;
    case "logs":
      return <LogsView endpoint={t.endpoint} containerId={t.containerId} active={active} />;
    case "exec":
      return <ExecView endpoint={t.endpoint} containerId={t.containerId} active={active} onTitle={() => {}} />;
    case "vault":
      return <VaultView />;
    case "settings":
      return <SettingsView />;
  }
}

export default function App() {
  const { tabs, activeTab, setActive, closeTab, openTab, sidebar, setSidebar, vault, refreshVault, refreshHosts, loadSettings, toast } = useApp();

  useEffect(() => {
    loadSettings();
    refreshHosts();
    refreshVault().then(async () => {
      const v = useApp.getState().vault;
      if (v.exists && !v.unlocked && v.remembered) {
        try {
          await api.vaultUnlockKeyring();
          await refreshVault();
        } catch {
          /* fall back to manual unlock */
        }
      }
    });
    const un = listen("vault-locked", () => {
      refreshVault();
      toast("Vault locked after inactivity");
    });
    return () => {
      un.then((f) => f());
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.shiftKey && e.code === "KeyT") {
        e.preventDefault();
        openTab({ kind: "local", title: "Local" });
      }
      if (e.ctrlKey && e.shiftKey && e.code === "KeyW" && activeTab) {
        e.preventDefault();
        closeTab(activeTab);
      }
      if (e.ctrlKey && e.code === "PageDown") {
        const i = tabs.findIndex((t) => t.id === activeTab);
        if (tabs.length) setActive(tabs[(i + 1) % tabs.length].id);
      }
      if (e.ctrlKey && e.code === "PageUp") {
        const i = tabs.findIndex((t) => t.id === activeTab);
        if (tabs.length) setActive(tabs[(i - 1 + tabs.length) % tabs.length].id);
      }
    };
    window.addEventListener("keydown", h, true);
    return () => window.removeEventListener("keydown", h, true);
  }, [tabs, activeTab, openTab, closeTab, setActive]);

  return (
    <div className="flex h-full">
      {/* Rail */}
      <nav className="flex w-12 flex-col items-center gap-1 border-r border-border bg-surface-1 py-2">
        <div className="mb-2 flex h-8 w-8 items-center justify-center rounded-lg bg-accent font-mono text-xs font-bold text-white">&gt;_</div>
        <RailButton title="Hosts" active={sidebar === "hosts"} onClick={() => setSidebar("hosts")}>
          <Server size={18} />
        </RailButton>
        <RailButton title="Docker" active={sidebar === "docker"} onClick={() => setSidebar("docker")}>
          <Box size={18} />
        </RailButton>
        <RailButton title="Vault" onClick={() => openTab({ kind: "vault", title: "Vault" }, { unique: true })}>
          <ShieldCheck size={18} />
        </RailButton>
        <div className="flex-1" />
        <RailButton
          title={vault.unlocked ? "Lock vault" : vault.exists ? "Unlock vault" : "Create vault"}
          onClick={async () => {
            if (vault.unlocked) {
              await api.vaultLock();
              await refreshVault();
              toast("Vault locked");
            } else if (vault.exists) useApp.getState().setUnlockPrompt(true);
            else openTab({ kind: "vault", title: "Vault" }, { unique: true });
          }}
        >
          {vault.unlocked ? <LockOpen size={18} className="text-ok" /> : <Lock size={18} />}
        </RailButton>
        <RailButton title="Settings" onClick={() => openTab({ kind: "settings", title: "Settings" }, { unique: true })}>
          <SettingsIcon size={18} />
        </RailButton>
      </nav>

      {/* Sidebar */}
      <aside className="w-72 shrink-0 border-r border-border bg-surface-1">{sidebar === "hosts" ? <HostsSidebar /> : <DockerSidebar />}</aside>

      {/* Workspace */}
      <main className="flex min-w-0 flex-1 flex-col">
        <div className="flex h-9 shrink-0 items-end gap-0.5 overflow-x-auto border-b border-border bg-surface-1 px-1">
          {tabs.map((t) => (
            <div
              key={t.id}
              onMouseDown={(e) => {
                if (e.button === 1) {
                  e.preventDefault();
                  closeTab(t.id);
                } else setActive(t.id);
              }}
              className={clsx(
                "group flex h-8 max-w-56 shrink-0 cursor-pointer items-center gap-2 rounded-t-md border border-b-0 px-3 text-xs",
                t.id === activeTab ? "border-border bg-surface-0 text-fg" : "border-transparent text-fg-muted hover:bg-surface-2",
              )}
            >
              <TabIcon t={t} />
              <span className="truncate">{t.title}</span>
              <button
                className="rounded p-0.5 opacity-50 hover:bg-surface-4 hover:opacity-100 cursor-pointer"
                onMouseDown={(e) => e.stopPropagation()}
                onClick={() => closeTab(t.id)}
                aria-label="Close tab"
              >
                <X size={12} />
              </button>
            </div>
          ))}
          <button
            title="New local terminal (Ctrl+Shift+T)"
            className="mb-1 ml-1 rounded p-1 text-fg-muted hover:bg-surface-3 hover:text-fg cursor-pointer"
            onClick={() => openTab({ kind: "local", title: "Local" })}
          >
            <Plus size={14} />
          </button>
        </div>
        <div className="relative min-h-0 flex-1">
          {tabs.length === 0 && (
            <Empty icon={<SquareTerminal size={40} />} title="Welcome to Shellport">
              Double-click a host to connect, open a local terminal with Ctrl+Shift+T, or manage containers from the Docker panel.
            </Empty>
          )}
          {tabs.map((t) => (
            <div key={t.id} className={clsx("absolute inset-0", t.id === activeTab ? "visible" : "invisible")}>
              <TabContent t={t} active={t.id === activeTab} />
            </div>
          ))}
        </div>
      </main>

      <HostEditor />
      <ImportDialog />
      <HostKeyDialog />
      <UnlockDialog />
      <Toasts />
    </div>
  );
}

function RailButton({ title, active, onClick, children }: { title: string; active?: boolean; onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      title={title}
      aria-label={title}
      onClick={onClick}
      className={clsx(
        "flex h-9 w-9 items-center justify-center rounded-lg cursor-pointer",
        active ? "bg-surface-3 text-fg" : "text-fg-muted hover:bg-surface-3 hover:text-fg",
      )}
    >
      {children}
    </button>
  );
}

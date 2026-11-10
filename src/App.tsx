import { listen } from "@tauri-apps/api/event";
import clsx from "clsx";
import { Box, Database, FolderOpen, PanelLeftClose, PanelLeftOpen, Lock, LockOpen, Plus, Server, Settings as SettingsIcon, ShieldCheck, SquareTerminal, Terminal, X } from "lucide-react";
import { useEffect, useState } from "react";
import { AuthPromptDialog, HostKeyDialog, Toasts, UnlockDialog, useAppErrors } from "./components/Dialogs";
import { DatabasesSidebar } from "./components/DatabasesSidebar";
import { DbEditor } from "./components/DbEditor";
import { ForwardsDialog } from "./components/ForwardsDialog";
import { HostEditor } from "./components/HostEditor";
import { DockerSidebar, HostsSidebar } from "./components/HostsSidebar";
import { ImportDialog } from "./components/ImportDialog";
import { TerminalView } from "./components/TerminalView";
import { Empty } from "./components/ui";
import { api } from "./lib/ipc";
import { onTabClosed, useApp, type Tab } from "./store";
import { DbView } from "./views/DbView";
import { DockerView, ExecView, LogsView, MultiLogsView } from "./views/DockerView";
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
    case "mlogs":
      return <Box size={s} />;
    case "db":
      return <Database size={s} />;
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
      return <SshTerminal tabId={t.id} hostId={t.hostId} active={active} />;
    case "sftp":
      return <SftpView hostId={t.hostId} active={active} />;
    case "docker":
      return <DockerView endpoint={t.endpoint} active={active} />;
    case "logs":
      return <LogsView endpoint={t.endpoint} containerId={t.containerId} active={active} />;
    case "exec":
      return <ExecView endpoint={t.endpoint} containerId={t.containerId} active={active} onTitle={() => {}} />;
    case "mlogs":
      return <MultiLogsView endpoint={t.endpoint} containers={t.containers} active={active} />;
    case "db":
      return <DbView tabId={t.id} dbId={t.dbId} active={active} />;
    case "vault":
      return <VaultView />;
    case "settings":
      return <SettingsView />;
  }
}

export default function App() {
  useAppErrors();
  useApp((s) => s.dbs);
  useApp((s) => s.hosts);
  const { tabs, activeTab, setActive, closeTab, openTab, sidebar, setSidebar, sidebarCollapsed, toggleSidebar, vault, refreshVault, refreshHosts, refreshDbs, loadSettings, toast } = useApp();

  useEffect(() => {
    Promise.all([loadSettings(), refreshHosts(), refreshDbs()]).then(() => useApp.getState().restoreSession());
    useApp.getState().refreshForwards();
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

  // Persist open tabs (debounced) once the previous session has been restored.
  const sessionReady = useApp((s) => s.sessionReady);
  useEffect(() => {
    if (!sessionReady) return;
    const t = setTimeout(() => api.sessionSave({ tabs, activeTab }).catch(() => {}), 400);
    return () => clearTimeout(t);
  }, [tabs, activeTab, sessionReady]);

  // Mount a tab's content only once it has been shown, so restored tabs connect lazily.
  const [visited, setVisited] = useState<Set<string>>(new Set());
  useEffect(() => {
    if (activeTab && !visited.has(activeTab)) setVisited((v) => new Set(v).add(activeTab));
  }, [activeTab, visited]);

  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.shiftKey && e.code === "KeyT") {
        e.preventDefault();
        openTab({ kind: "local", title: "Local" });
      }
      if (e.ctrlKey && e.shiftKey && e.code === "KeyB") {
        e.preventDefault();
        toggleSidebar();
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
  }, [tabs, activeTab, openTab, closeTab, setActive, toggleSidebar]);

  return (
    <div className="flex h-full">
      {/* Rail */}
      <nav className="flex w-12 flex-col items-center gap-1 border-r border-border bg-surface-1 py-2">
        <img src="/logo.svg" alt="Shellport" className="mb-2 h-9 w-9" draggable={false} />
        <RailButton title="Hosts" active={!sidebarCollapsed && sidebar === "hosts"} onClick={() => (!sidebarCollapsed && sidebar === "hosts" ? toggleSidebar() : setSidebar("hosts"))}>
          <Server size={18} />
        </RailButton>
        <RailButton title="Databases" active={!sidebarCollapsed && sidebar === "databases"} onClick={() => (!sidebarCollapsed && sidebar === "databases" ? toggleSidebar() : setSidebar("databases"))}>
          <Database size={18} />
        </RailButton>
        <RailButton title="Docker" active={!sidebarCollapsed && sidebar === "docker"} onClick={() => (!sidebarCollapsed && sidebar === "docker" ? toggleSidebar() : setSidebar("docker"))}>
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
      {!sidebarCollapsed && (
        <aside className="w-72 shrink-0 border-r border-border bg-surface-1">{sidebar === "hosts" ? <HostsSidebar /> : sidebar === "databases" ? <DatabasesSidebar /> : <DockerSidebar />}</aside>
      )}

      {/* Workspace */}
      <main className="flex min-w-0 flex-1 flex-col">
        <div className="flex h-9 shrink-0 items-end gap-0.5 overflow-x-auto border-b border-border bg-surface-1 px-1">
          <button
            title={sidebarCollapsed ? "Show sidebar (Ctrl+Shift+B)" : "Hide sidebar (Ctrl+Shift+B)"}
            aria-label={sidebarCollapsed ? "Show sidebar" : "Hide sidebar"}
            className="mb-1 mr-1 shrink-0 rounded p-1 text-fg-muted hover:bg-surface-3 hover:text-fg cursor-pointer"
            onClick={toggleSidebar}
          >
            {sidebarCollapsed ? <PanelLeftOpen size={14} /> : <PanelLeftClose size={14} />}
          </button>
          {tabs.map((t) => (
            <div
              key={t.id}
              style={tabColor(t) ? { boxShadow: `inset 0 2px 0 ${tabColor(t)}` } : undefined}
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
              <span className="flex shrink-0">
                <TabIcon t={t} />
              </span>
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
            <Empty icon={<img src="/logo.svg" alt="" className="h-14 w-14" draggable={false} />} title="Welcome to Shellport">
              Double-click a host to connect, open a local terminal with Ctrl+Shift+T, or manage containers from the Docker panel.
            </Empty>
          )}
          {tabs.map((t) => (
            <div key={t.id} className={clsx("absolute inset-0", t.id === activeTab ? "visible" : "invisible")}>
              {(visited.has(t.id) || t.id === activeTab) && <TabContent t={t} active={t.id === activeTab} />}
            </div>
          ))}
        </div>
      </main>

      <HostEditor />
      <DbEditor />
      <ImportDialog />
      <HostKeyDialog />
      <AuthPromptDialog />
      <ForwardsDialog />
      <UnlockDialog />
      <Toasts />
    </div>
  );
}

/** An SSH terminal tab: reconnect policy and tmux session come from the host. */
function SshTerminal({ tabId, hostId, active }: { tabId: string; hostId: string; active: boolean }) {
  const host = useApp((s) => s.hosts.find((h) => h.id === hostId));
  const closeTab = useApp((s) => s.closeTab);
  const tmux = !!host?.tmuxSession;
  // Closing the tab (not reconnecting or quitting) ends its tmux session too.
  useEffect(() => {
    if (!tmux) return;
    onTabClosed(tabId, () => api.termKillTmux(hostId, tabId).catch(() => {}));
    return () => onTabClosed(tabId, null);
  }, [tmux, tabId, hostId]);
  return (
    <TerminalView
      active={active}
      open={(c, r, ch) => api.termOpenSsh(hostId, c, r, ch, tabId)}
      onLost={host?.onDisconnect ?? "reconnect"}
      probeHostId={hostId}
      onCloseTab={() => closeTab(tabId)}
    />
  );
}

/** The colour of the host or database a tab belongs to, shown as a stripe so production stands out. */
function tabColor(t: Tab): string | null {
  const { hosts, dbs } = useApp.getState();
  if (t.kind === "db") return dbs.find((d) => d.id === t.dbId)?.color ?? null;
  if ("hostId" in t) return hosts.find((h) => h.id === t.hostId)?.color ?? null;
  return null;
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

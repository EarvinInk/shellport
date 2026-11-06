import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  ContainerInfo,
  DockerAction,
  EndpointInfo,
  FileEntry,
  Host,
  ImageInfo,
  SecretInput,
  SecretMeta,
  Settings,
  SshConfigEntry,
  TermEvent,
  TransferEvent,
  VaultStatus,
} from "./types";

export type Opener = (cols: number, rows: number, ch: Channel<TermEvent>) => Promise<string>;

export const api = {
  // vault
  vaultStatus: () => invoke<VaultStatus>("vault_status"),
  vaultCreate: (password: string, remember: boolean) => invoke<VaultStatus>("vault_create", { password, remember }),
  vaultUnlock: (password: string, remember: boolean) => invoke<VaultStatus>("vault_unlock", { password, remember }),
  vaultUnlockKeyring: () => invoke<VaultStatus>("vault_unlock_keyring"),
  vaultLock: () => invoke<VaultStatus>("vault_lock"),
  vaultForgetDevice: () => invoke<VaultStatus>("vault_forget_device"),
  vaultChangePassword: (old: string, next: string) => invoke<void>("vault_change_password", { old, new: next }),
  secretsList: () => invoke<SecretMeta[]>("secrets_list"),
  secretSave: (input: SecretInput) => invoke<SecretMeta>("secret_save", { input }),
  secretDelete: (id: string) => invoke<void>("secret_delete", { id }),
  secretReveal: (id: string) => invoke<{ value: string; passphrase?: string | null }>("secret_reveal", { id }),
  secretPublicKey: (id: string) => invoke<string>("secret_public_key", { id }),
  secretGenerateKey: (name: string) => invoke<SecretMeta>("secret_generate_key", { name }),

  // hosts
  hostsList: () => invoke<Host[]>("hosts_list"),
  hostSave: (host: Host) => invoke<Host>("host_save", { host }),
  hostDelete: (id: string) => invoke<void>("host_delete", { id }),
  hostDisconnect: (id: string) => invoke<void>("host_disconnect", { id }),
  hostTest: (id: string) => invoke<string>("host_test", { id }),
  hostkeyRespond: (requestId: string, accept: boolean) => invoke<void>("hostkey_respond", { requestId, accept }),
  sshConfigPreview: (path?: string) => invoke<SshConfigEntry[]>("ssh_config_preview", { path }),
  sshConfigImport: (aliases: string[], path?: string) => invoke<number>("ssh_config_import", { aliases, path }),
  forwardStart: (hostId: string, forwardId: string) => invoke<string>("forward_start", { hostId, forwardId }),
  taskCancel: (taskId: string) => invoke<boolean>("task_cancel", { taskId }),
  tasksActive: () => invoke<string[]>("tasks_active"),

  // terminals
  termOpenLocal: (cols: number, rows: number, onEvent: Channel<TermEvent>, cwd?: string) =>
    invoke<string>("term_open_local", { cols, rows, cwd, onEvent }),
  termOpenSsh: (hostId: string, cols: number, rows: number, onEvent: Channel<TermEvent>) =>
    invoke<string>("term_open_ssh", { hostId, cols, rows, onEvent }),
  termWrite: (id: string, data: string) => invoke<void>("term_write", { id, data }),
  termWriteB64: (id: string, data: string) => invoke<void>("term_write_b64", { id, data }),
  termResize: (id: string, cols: number, rows: number) => invoke<void>("term_resize", { id, cols, rows }),
  termClose: (id: string) => invoke<void>("term_close", { id }),

  // sftp
  sftpHome: (hostId: string) => invoke<string>("sftp_home", { hostId }),
  sftpList: (hostId: string, path: string) => invoke<FileEntry[]>("sftp_list", { hostId, path }),
  localList: (path: string) => invoke<FileEntry[]>("local_list", { path }),
  localHome: () => invoke<string>("local_home"),
  localReadText: (path: string) => invoke<string>("local_read_text", { path }),
  sftpMkdir: (hostId: string, path: string) => invoke<void>("sftp_mkdir", { hostId, path }),
  sftpRename: (hostId: string, from: string, to: string) => invoke<void>("sftp_rename", { hostId, from, to }),
  sftpRemove: (hostId: string, path: string) => invoke<void>("sftp_remove", { hostId, path }),
  sftpChmod: (hostId: string, path: string, mode: number) => invoke<void>("sftp_chmod", { hostId, path, mode }),
  sftpDownload: (hostId: string, remote: string, localDir: string, onEvent: Channel<TransferEvent>) =>
    invoke<void>("sftp_download", { hostId, remote, localDir, onEvent }),
  sftpUpload: (hostId: string, local: string, remoteDir: string, onEvent: Channel<TransferEvent>) =>
    invoke<void>("sftp_upload", { hostId, local, remoteDir, onEvent }),

  // docker
  dockerInfo: (endpoint: string) => invoke<EndpointInfo>("docker_info", { endpoint }),
  dockerList: (endpoint: string, all: boolean) => invoke<ContainerInfo[]>("docker_list", { endpoint, all }),
  dockerImages: (endpoint: string) => invoke<ImageInfo[]>("docker_images", { endpoint }),
  dockerAction: (endpoint: string, id: string, action: DockerAction) =>
    invoke<void>("docker_action", { endpoint, id, action }),
  dockerInspect: (endpoint: string, id: string) => invoke<unknown>("docker_inspect", { endpoint, id }),
  dockerLogs: (endpoint: string, id: string, tail: number, timestamps: boolean, follow: boolean, onEvent: Channel<TermEvent>) =>
    invoke<string>("docker_logs", { endpoint, id, tail, timestamps, follow, onEvent }),
  dockerExec: (endpoint: string, id: string, cols: number, rows: number, onEvent: Channel<TermEvent>, shell?: string, user?: string) =>
    invoke<string>("docker_exec", { endpoint, id, shell, user, cols, rows, onEvent }),
  dockerReset: (endpoint: string) => invoke<void>("docker_reset", { endpoint }),

  // settings
  settingsGet: () => invoke<Settings>("settings_get"),
  settingsSave: (settings: Settings) => invoke<Settings>("settings_save", { settings }),
};

export function b64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

export function binaryStringToB64(s: string): string {
  return btoa(s);
}

export function errMsg(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return JSON.stringify(e);
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v < 10 ? 1 : 0)} ${units[i]}`;
}

export function modeString(mode?: number | null, isDir = false): string {
  if (mode == null) return "";
  const t = isDir ? "d" : "-";
  const r = (m: number, s: number) =>
    (m & (4 << s) ? "r" : "-") + (m & (2 << s) ? "w" : "-") + (m & (1 << s) ? "x" : "-");
  return t + r(mode, 6) + r(mode, 3) + r(mode, 0);
}

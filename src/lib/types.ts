export type SecretKind = "password" | "private_key" | "token" | "note";

export interface SecretMeta {
  id: string;
  name: string;
  kind: SecretKind;
  username?: string | null;
  notes: string;
  hasPassphrase: boolean;
  updatedAt: number;
}

export interface SecretInput {
  id?: string | null;
  name: string;
  kind: SecretKind;
  username?: string | null;
  value?: string | null;
  passphrase?: string | null;
  notes: string;
}

export interface VaultStatus {
  exists: boolean;
  unlocked: boolean;
  remembered: boolean;
}

export type AuthMethod =
  | { type: "password"; secretId?: string | null }
  | { type: "vault_key"; secretId?: string | null }
  | { type: "key_file"; path: string; passphraseSecretId?: string | null }
  | { type: "agent" };

export interface PortForward {
  id: string;
  localPort: number;
  remoteHost: string;
  remotePort: number;
  autoStart: boolean;
}

export interface Host {
  id: string;
  name: string;
  group: string;
  hostname: string;
  port: number;
  username: string;
  auth: AuthMethod;
  jumpHostId?: string | null;
  dockerSocket?: string | null;
  tags: string[];
  color?: string | null;
  forwards: PortForward[];
  keepaliveSecs?: number | null;
  startupCommand?: string | null;
}

export interface SshConfigEntry {
  alias: string;
  hostname?: string | null;
  user?: string | null;
  port?: number | null;
  identityFile?: string | null;
  proxyJump?: string | null;
}

export type TermEvent =
  | { type: "data"; data: string }
  | { type: "exit"; code?: number | null }
  | { type: "error"; message: string };

export interface FileEntry {
  name: string;
  path: string;
  isDir: boolean;
  isSymlink: boolean;
  size: number;
  mtime?: number | null;
  permissions?: number | null;
}

export type TransferEvent =
  | { type: "progress"; file: string; bytes: number; total: number; filesDone: number }
  | { type: "done"; files: number; bytes: number }
  | { type: "error"; message: string };

export interface ContainerInfo {
  id: string;
  name: string;
  image: string;
  state: string;
  status: string;
  created: number;
  ports: string[];
  project?: string | null;
}

export interface ImageInfo {
  id: string;
  tags: string[];
  size: number;
  created: number;
  containers: number;
}

export interface EndpointInfo {
  endpoint: string;
  mode: "local" | "ssh-socket" | "ssh-cli";
  serverVersion?: string | null;
}

export type DockerAction = "start" | "stop" | "restart" | "pause" | "unpause" | "kill" | "remove";

export interface Settings {
  theme: string;
  fontFamily: string;
  fontSize: number;
  scrollback: number;
  cursorStyle: "block" | "underline" | "bar";
  cursorBlink: boolean;
  copyOnSelect: boolean;
  autoLockMinutes: number;
  localShell: string;
  dockerLogTail: number;
}

export interface HostKeyPrompt {
  requestId: string;
  host: string;
  port: number;
  algorithm: string;
  fingerprint: string;
  changed: boolean;
  changedLine?: number | null;
}

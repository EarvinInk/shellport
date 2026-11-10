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
  /** What a terminal does when the connection drops. */
  onDisconnect?: "reconnect" | "ask" | "close";
  /** Keep each terminal's shell alive in a tmux session on the server. */
  tmuxSession?: boolean;
  /** Production server: risky actions always ask first. */
  production?: boolean;
  /** Markdown notes about this host. */
  notes?: string | null;
  /** Kept in sync with this `Host` entry in ~/.ssh/config. */
  sshConfigAlias?: string | null;
}

export interface HostStatus {
  id: string;
  /** null: not checked (behind a jump host that is up). */
  up: boolean | null;
}

export interface HostImportPreview {
  names: string[];
  existing: string[];
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
  | { type: "error"; message: string }
  | { type: "lost"; reason: string };

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
  restoreTabs: boolean;
  hostStatusChecks: boolean;
}

export interface AuthPrompt {
  requestId: string;
  host: string;
  name: string;
  instructions: string;
  prompts: { prompt: string; echo: boolean }[];
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

export interface UsageRow {
  kind: string;
  total: number;
  active: number;
  size?: number | null;
  reclaimable?: number | null;
  sizeText?: string | null;
  reclaimableText?: string | null;
}

export type PruneTarget = "containers" | "images" | "volumes" | "networks" | "build_cache" | "system";

export interface PruneOptions {
  all: boolean;
  volumes: boolean;
}

export interface PruneResult {
  removed: number;
  reclaimed?: number | null;
  reclaimedText?: string | null;
  details: string[];
}

export interface VolumeInfo {
  name: string;
  driver: string;
  mountpoint: string;
  created?: string | null;
  project?: string | null;
  anonymous: boolean;
  usedBy: string[];
}

export interface NetworkInfo {
  id: string;
  name: string;
  driver: string;
  scope: string;
  subnet?: string | null;
  gateway?: string | null;
  internal: boolean;
  project?: string | null;
  builtin: boolean;
  containers: string[];
}

// ---------- databases ----------

/** Must match `db::DbEngine` in the backend. */
export type DbEngine = "postgres";

export type SslMode = "disable" | "prefer" | "require" | "verify-full";

/** How carefully queries on a connection are treated. */
export type DbSafety = "off" | "confirm" | "read-only";

export type TxState = "idle" | "open" | "failed";

export interface DbProfile {
  id: string;
  name: string;
  group: string;
  engine: DbEngine;
  host: string;
  port: number;
  database: string;
  username: string;
  passwordSecretId?: string | null;
  viaHostId?: string | null;
  sslMode: SslMode;
  safety: DbSafety;
  production?: boolean;
  options: Record<string, string>;
  color?: string | null;
  tags: string[];
}

export interface EngineInfo {
  engine: DbEngine;
  label: string;
  defaultPort: number;
  capabilities: { cancel: boolean; transactions: boolean; tls: boolean };
}

export interface SchemaNode {
  name: string;
  path: string[];
  kind: string;
  detail?: string | null;
  hasChildren: boolean;
  previewSql?: string | null;
  insertText?: string | null;
  system: boolean;
}

export type Cell =
  | { t: "null" }
  | { t: "bool"; v: boolean }
  | { t: "int"; v: number }
  | { t: "float"; v: number }
  | { t: "text"; v: string }
  | { t: "json"; v: string }
  | { t: "bytes"; v: string };

export interface StatementResult {
  columns: { name: string; typeName: string }[];
  rows: Cell[][];
  rowsAffected?: number | null;
  truncated: boolean;
}

export interface QueryOutput {
  results: StatementResult[];
  notices: string[];
  elapsedMs: number;
  tx: TxState;
}

export interface QueryRisk {
  statement: string;
  reason: string;
}

export type QueryOutcome = { kind: "done"; output: QueryOutput } | { kind: "needsConfirmation"; risks: QueryRisk[] };

export interface DbConnected {
  sessionId: string;
  serverVersion: string;
  engine: DbEngine;
  safety: DbSafety;
  /** The connection or its SSH host is marked Production. */
  production: boolean;
}

export interface DbUrl {
  host: string;
  port?: number | null;
  database: string;
  username: string;
  password?: string | null;
  sslMode?: SslMode | null;
  options: Record<string, string>;
}

// ---------- backup ----------

export interface BackupCounts {
  total: number;
  new: number;
}

export interface BackupSummary {
  createdAt: number;
  app: string;
  hosts: BackupCounts;
  databases: BackupCounts;
  secrets?: BackupCounts | null;
  vaultExists: boolean;
  vaultUnlocked: boolean;
}

export interface BackupRestored {
  hosts: number;
  databases: number;
  secrets: number;
  settings: boolean;
}

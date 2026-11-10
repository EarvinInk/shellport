import type { DbEngine, SslMode } from "./types";

/** An engine-specific setting, stored in `DbProfile.options[key]`. */
export interface EngineField {
  key: string;
  label: string;
  placeholder?: string;
  hint?: string;
}

export interface EngineUi {
  id: DbEngine;
  label: string;
  defaultPort: number;
  /** Database name used when the field is left empty (shown as placeholder). */
  databasePlaceholder: string;
  usernamePlaceholder: string;
  /** TLS modes this engine supports; empty hides the TLS setting. */
  sslModes: SslMode[];
  extraFields: EngineField[];
  /** Starter text for a new query editor. */
  sampleQuery: string;
}

/**
 * Frontend side of the engine registry (backend: `db/registry.rs`).
 * Adding an engine = one entry here + one driver in the backend.
 */
export const DB_ENGINES: Record<DbEngine, EngineUi> = {
  postgres: {
    id: "postgres",
    label: "PostgreSQL",
    defaultPort: 5432,
    databasePlaceholder: "same as user",
    usernamePlaceholder: "postgres",
    sslModes: ["disable", "prefer", "require", "verify-full"],
    extraFields: [{ key: "application_name", label: "Application name", placeholder: "Shellport", hint: "Shown in pg_stat_activity" }],
    sampleQuery: "SELECT now(), current_user, version();",
  },
};

export const SSL_LABELS: Record<SslMode, string> = {
  disable: "Disable (no TLS)",
  prefer: "Prefer (TLS if offered, not verified)",
  require: "Require (TLS, not verified)",
  "verify-full": "Verify full (TLS, verified certificate)",
};

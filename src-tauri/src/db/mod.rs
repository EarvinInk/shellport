//! Database client: engine-neutral traits and types.
//!
//! Every engine lives in `drivers/<engine>.rs` and implements [`DbDriver`] and
//! [`DbSession`]; `registry.rs` is the only place that names the engines.
//! Drivers never deal with SSH: when a profile goes through a host, the
//! session layer opens a tunnel first and hands the driver a plain local
//! `connect_host:connect_port` (see `session.rs`).
//!
//! Adding an engine:
//! 1. add a [`DbEngine`] variant,
//! 2. implement the driver in `drivers/<engine>.rs`,
//! 3. register it in `registry::driver`,
//! 4. add its entry to `src/lib/dbEngines.ts` in the frontend.

pub mod drivers;
pub mod registry;
pub mod risk;
pub mod session;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum DbEngine {
    Postgres,
}

impl DbEngine {
    pub const ALL: &'static [DbEngine] = &[DbEngine::Postgres];
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SslMode {
    Disable,
    /// Use TLS when the server offers it, without verifying the certificate.
    #[default]
    Prefer,
    /// Require TLS, without verifying the certificate.
    Require,
    /// Require TLS and verify the certificate chain and host name.
    VerifyFull,
}

/// What an engine supports, so the UI can hide what does not apply.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub cancel: bool,
    pub transactions: bool,
    pub tls: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineInfo {
    pub engine: DbEngine,
    pub label: &'static str,
    pub default_port: u16,
    pub capabilities: Capabilities,
}

/// Everything a driver needs to connect. `connect_host:connect_port` is where
/// the TCP connection goes (a local tunnel end when going through SSH);
/// `host` is the server's real name, used for TLS verification and display.
pub struct ConnectParams {
    pub host: String,
    pub connect_host: String,
    pub connect_port: u16,
    pub database: String,
    pub username: String,
    pub password: Option<Zeroizing<String>>,
    pub ssl_mode: SslMode,
    /// Engine-specific settings from the profile (e.g. `application_name`).
    pub options: BTreeMap<String, String>,
    /// Ask the server to refuse writes for this session.
    pub read_only: bool,
}

/// A node in the lazily loaded schema tree. `path` identifies it when asking
/// for its children; each engine decides what the levels mean.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SchemaNode {
    pub name: String,
    pub path: Vec<String>,
    /// e.g. "schema", "table", "view", "column", "function".
    pub kind: String,
    /// Extra text shown next to the name (a column type, row estimate…).
    pub detail: Option<String>,
    pub has_children: bool,
    /// For tables and views: a ready-to-run query that previews the data.
    pub preview_sql: Option<String>,
    /// The node's name quoted for use in SQL (e.g. `"shop"."orders"`).
    pub insert_text: Option<String>,
    /// Built-in objects (system schemas) the UI shows dimmed.
    pub system: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ColumnMeta {
    pub name: String,
    pub type_name: String,
}

/// One result value. Engines map their native types onto these.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "t", content = "v", rename_all = "lowercase")]
pub enum Cell {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Json(String),
    Bytes(String),
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatementResult {
    pub columns: Vec<ColumnMeta>,
    pub rows: Vec<Vec<Cell>>,
    /// Rows affected (or returned) as reported by the server.
    pub rows_affected: Option<u64>,
    /// More rows existed than the row limit allowed.
    pub truncated: bool,
}

/// Whether the session is inside an explicit transaction block.
#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TxState {
    #[default]
    Idle,
    /// `BEGIN` was run and not yet committed or rolled back.
    Open,
    /// A statement failed inside the transaction; only ROLLBACK works now.
    Failed,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryOutput {
    pub results: Vec<StatementResult>,
    pub notices: Vec<String>,
    pub elapsed_ms: u64,
    /// Transaction state after the query ran.
    pub tx: TxState,
}

#[async_trait]
pub trait DbDriver: Send + Sync {
    fn info(&self) -> EngineInfo;
    async fn connect(&self, params: ConnectParams) -> Result<Arc<dyn DbSession>>;
}

#[async_trait]
pub trait DbSession: Send + Sync {
    fn server_version(&self) -> String;
    /// Children of the node at `path`; the empty path lists the top level.
    async fn children(&self, path: &[String]) -> Result<Vec<SchemaNode>>;
    /// Run one or more statements, keeping at most `row_limit` rows each.
    async fn query(&self, sql: &str, row_limit: usize) -> Result<QueryOutput>;
    /// Ask the server to stop the running query.
    async fn cancel(&self) -> Result<()>;
    async fn close(&self);
    /// The connection to the server is gone (network drop, server restart).
    fn is_closed(&self) -> bool;
    /// Current transaction state (see [`TxState`]).
    async fn tx_state(&self) -> Result<TxState>;
}

/// Prefix of the error returned when a session's connection has dropped;
/// the UI matches on it to offer a reconnect.
pub const CONNECTION_LOST: &str = "connection lost";

/// Quote an SQL identifier with double quotes (Postgres and the SQL standard).
pub fn quote_ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_ident_escapes_quotes() {
        assert_eq!(quote_ident("users"), "\"users\"");
        assert_eq!(quote_ident("we\"ird"), "\"we\"\"ird\"");
    }

    #[test]
    fn cell_serializes_tagged() {
        assert_eq!(
            serde_json::to_string(&Cell::Int(3)).unwrap(),
            r#"{"t":"int","v":3}"#
        );
        assert_eq!(
            serde_json::to_string(&Cell::Null).unwrap(),
            r#"{"t":"null"}"#
        );
    }

    #[test]
    fn ssl_mode_names() {
        assert_eq!(
            serde_json::to_string(&SslMode::VerifyFull).unwrap(),
            r#""verify-full""#
        );
        assert_eq!(
            serde_json::from_str::<SslMode>(r#""prefer""#).unwrap(),
            SslMode::Prefer
        );
    }
}

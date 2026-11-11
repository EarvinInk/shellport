//! PostgreSQL driver (tokio-postgres + rustls/ring).
//!
//! Data goes over the simple-query (text) protocol, which runs any script
//! (several statements, DDL, `BEGIN`…`COMMIT`) the way psql does. For a single
//! statement the query is also prepared, only to learn the column types, so
//! numbers, booleans and JSON come back typed. A single plain SELECT is read
//! through a server-side cursor so only `row_limit` rows ever leave the server.
//!
//! Behind a transaction-mode pooler (PgBouncer, Supabase) named prepared
//! statements break, so "pooler mode" skips them: schema lookups always use
//! unnamed statements (`query_typed`), and query results come back as text.

use crate::db::{
    quote_ident, risk, Capabilities, Cell, ColumnMeta, ConnectParams, DbDriver, DbEngine, DbSession, EngineInfo, QueryOutput, SchemaNode, SslMode,
    StatementResult, TxState,
};
use std::sync::atomic::{AtomicBool, Ordering};
use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use futures_util::{pin_mut, StreamExt};
use parking_lot::Mutex;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::AbortHandle;
use tokio_postgres::types::Type;
use tokio_postgres::{AsyncMessage, Client, SimpleQueryMessage};
use tokio_postgres_rustls::MakeRustlsConnect;

pub struct Postgres;

#[async_trait]
impl DbDriver for Postgres {
    fn info(&self) -> EngineInfo {
        EngineInfo {
            engine: DbEngine::Postgres,
            label: "PostgreSQL",
            default_port: 5432,
            capabilities: Capabilities { cancel: true, transactions: true, tls: true },
        }
    }

    async fn connect(&self, p: ConnectParams) -> Result<Arc<dyn DbSession>> {
        if p.connect_host.starts_with('/') {
            bail!("unix socket connections are not supported; use a host name");
        }
        let mut cfg = tokio_postgres::Config::new();
        // `host` names the server (TLS checks it); `hostaddr` is where the
        // TCP connection actually goes, e.g. the local end of an SSH tunnel.
        cfg.host(&p.host);
        if let Ok(ip) = p.connect_host.parse() {
            cfg.hostaddr(ip);
        } else if p.connect_host != p.host {
            cfg.host(&p.connect_host);
        }
        cfg.port(p.connect_port)
            .user(&p.username)
            .dbname(if p.database.is_empty() { &p.username } else { &p.database })
            .application_name(p.options.get("application_name").map(String::as_str).unwrap_or("Shellport"))
            .connect_timeout(Duration::from_secs(15))
            .ssl_mode(match p.ssl_mode {
                SslMode::Disable => tokio_postgres::config::SslMode::Disable,
                SslMode::Prefer => tokio_postgres::config::SslMode::Prefer,
                SslMode::Require | SslMode::VerifyFull => tokio_postgres::config::SslMode::Require,
            });
        if let Some(pw) = &p.password {
            cfg.password(pw.as_bytes());
        }
        // Notice dead connections (sleep, NAT timeouts) within about a minute.
        cfg.keepalives(true)
            .keepalives_idle(Duration::from_secs(30))
            .keepalives_interval(Duration::from_secs(10))
            .keepalives_retries(3);
        let pooler = p.options.get("pooler").is_some_and(|v| v == "1" || v == "true");
        // Poolers reject startup options, so read-only is then checked by us
        // before each query instead (see `query`).
        if p.read_only && !pooler {
            cfg.options("-c default_transaction_read_only=on");
        }
        let tls = tls_connector(p.ssl_mode == SslMode::VerifyFull)?;
        let (client, mut conn) = match cfg.connect(tls.clone()).await {
            Ok(c) => c,
            // A pooler that doesn't pass startup options through: connect again
            // in pooler mode, where read-only is checked on our side.
            Err(e) if p.read_only && !pooler && e.as_db_error().is_some_and(|d| d.message().contains("unsupported startup parameter")) => {
                let mut p = p;
                p.options.insert("pooler".into(), "1".into());
                return Box::pin(self.connect(p)).await;
            }
            Err(e) => return Err(pg_err(e)),
        };

        // Drive the connection and collect NOTICE/WARNING messages.
        let notices: Arc<Mutex<Vec<String>>> = Default::default();
        let n2 = notices.clone();
        let task = tokio::spawn(async move {
            let messages = futures_util::stream::poll_fn(move |cx| conn.poll_message(cx));
            pin_mut!(messages);
            while let Some(m) = messages.next().await {
                match m {
                    Ok(AsyncMessage::Notice(n)) => n2.lock().push(format!("{}: {}", n.severity(), n.message())),
                    Ok(_) => {}
                    Err(e) => {
                        log::warn!("postgres connection closed: {e}");
                        break;
                    }
                }
            }
        });

        let version = match client.simple_query("SHOW server_version").await {
            Ok(msgs) => msgs
                .iter()
                .find_map(|m| match m {
                    SimpleQueryMessage::Row(r) => r.get(0).map(|v| format!("PostgreSQL {v}")),
                    _ => None,
                })
                .unwrap_or_else(|| "PostgreSQL".into()),
            Err(e) => {
                task.abort();
                return Err(pg_err(e));
            }
        };
        Ok(Arc::new(PgSession {
            client,
            tls,
            notices,
            conn_task: task.abort_handle(),
            version,
            pooler: AtomicBool::new(pooler),
            read_only_client_check: p.read_only && pooler,
        }))
    }
}

struct PgSession {
    client: Client,
    tls: MakeRustlsConnect,
    notices: Arc<Mutex<Vec<String>>>,
    conn_task: AbortHandle,
    version: String,
    /// Avoid named prepared statements (transaction-mode pooler).
    pooler: AtomicBool,
    /// Read-only the server can't enforce for us (pooler mode).
    read_only_client_check: bool,
}

impl PgSession {
    /// Rows of a lookup query, as text, using an unnamed statement (works
    /// behind poolers). Parameters are text.
    async fn lookup(&self, sql: &str, params: &[&str]) -> Result<Vec<Vec<Option<String>>>> {
        let p: Vec<(&(dyn tokio_postgres::types::ToSql + Sync), Type)> =
            params.iter().map(|v| (v as &(dyn tokio_postgres::types::ToSql + Sync), Type::TEXT)).collect();
        let rows = self.client.query_typed(sql, &p).await.map_err(pg_err)?;
        Ok(rows.iter().map(|r| (0..r.len()).map(|i| r.get::<_, Option<String>>(i)).collect()).collect())
    }

    /// Column types of a single statement, when it can be prepared.
    async fn column_types(&self, sql: &str) -> Vec<Type> {
        if self.pooler.load(Ordering::Relaxed) {
            return Vec::new();
        }
        match self.client.prepare(sql).await {
            Ok(st) => st.columns().iter().map(|c| c.type_().clone()).collect(),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("prepared statement") && (msg.contains("does not exist") || msg.contains("already exists")) {
                    log::info!("named prepared statements unsupported here (pooler?); switching to pooler mode");
                    self.pooler.store(true, Ordering::Relaxed);
                }
                Vec::new()
            }
        }
    }
}

impl Drop for PgSession {
    fn drop(&mut self) {
        self.conn_task.abort();
    }
}

const SYSTEM_SCHEMAS: &str = "('pg_catalog', 'information_schema')";

#[async_trait]
impl DbSession for PgSession {
    fn server_version(&self) -> String {
        self.version.clone()
    }

    async fn children(&self, path: &[String]) -> Result<Vec<SchemaNode>> {
        let node = |name: String, path: Vec<String>, kind: &str, detail: Option<String>, has_children: bool, preview_sql: Option<String>| SchemaNode {
            name,
            path,
            kind: kind.into(),
            detail,
            has_children,
            preview_sql,
            insert_text: None,
            system: false,
        };
        match path {
            [] => {
                let sql = format!(
                    "SELECT nspname::text FROM pg_namespace \
                     WHERE nspname NOT LIKE 'pg\\_toast%' AND nspname NOT LIKE 'pg\\_temp%' \
                     ORDER BY nspname IN {SYSTEM_SCHEMAS}, nspname"
                );
                let rows = self.lookup(&sql, &[]).await?;
                Ok(rows
                    .iter()
                    .map(|r| {
                        let s = col(r, 0);
                        let mut n = node(s.clone(), vec![s.clone()], "schema", None, true, None);
                        n.system = s == "pg_catalog" || s == "information_schema";
                        n.insert_text = Some(quote_ident(&s));
                        n
                    })
                    .collect())
            }
            [schema] => Ok(["tables", "views", "functions"]
                .iter()
                .map(|f| node(capitalize(f), vec![schema.clone(), f.to_string()], "folder", None, true, None))
                .collect()),
            [schema, folder] if folder == "tables" || folder == "views" => {
                let kinds = if folder == "tables" { "('r','p','f')" } else { "('v','m')" };
                let sql = format!(
                    "SELECT c.relname::text, c.relkind::text, c.reltuples::bigint::text FROM pg_class c \
                     JOIN pg_namespace n ON n.oid = c.relnamespace \
                     WHERE n.nspname = $1 AND c.relkind IN {kinds} ORDER BY c.relname"
                );
                let rows = self.lookup(&sql, &[schema.as_str()]).await?;
                Ok(rows
                    .iter()
                    .map(|r| {
                        let name = col(r, 0);
                        let kind = col(r, 1);
                        let est: i64 = col(r, 2).parse().unwrap_or(-1);
                        let detail = match kind.as_str() {
                            "m" => Some("materialized".to_string()),
                            "p" => Some("partitioned".to_string()),
                            "f" => Some("foreign".to_string()),
                            _ if est >= 0 && folder == "tables" => Some(format!("~{est} rows")),
                            _ => None,
                        };
                        let preview = format!("SELECT * FROM {}.{} LIMIT 100;", quote_ident(schema), quote_ident(&name));
                        let k = if folder == "tables" { "table" } else { "view" };
                        let mut n = node(name.clone(), vec![schema.clone(), folder.clone(), name.clone()], k, detail, true, Some(preview));
                        n.insert_text = Some(format!("{}.{}", quote_ident(schema), quote_ident(&name)));
                        n
                    })
                    .collect())
            }
            [schema, folder] if folder == "functions" => {
                let sql = "SELECT p.proname::text, pg_get_function_identity_arguments(p.oid), \
                           CASE p.prokind WHEN 'p' THEN 'procedure' ELSE 'function' END \
                           FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
                           WHERE n.nspname = $1 AND p.prokind IN ('f','p') ORDER BY 1, 2 LIMIT 1000";
                let rows = self.lookup(sql, &[schema.as_str()]).await?;
                Ok(rows
                    .iter()
                    .map(|r| {
                        let name = col(r, 0);
                        let args = col(r, 1);
                        let kind = col(r, 2);
                        let mut n = node(format!("{name}({args})"), vec![schema.clone(), folder.clone(), name.clone()], &kind, None, false, None);
                        n.insert_text = Some(format!("{}.{}", quote_ident(schema), quote_ident(&name)));
                        n
                    })
                    .collect())
            }
            [schema, _, rel] => {
                let sql = "SELECT a.attname::text, format_type(a.atttypid, a.atttypmod), a.attnotnull::text, \
                           EXISTS (SELECT 1 FROM pg_index i WHERE i.indrelid = c.oid AND i.indisprimary AND a.attnum = ANY(i.indkey))::text \
                           FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid \
                           JOIN pg_namespace n ON n.oid = c.relnamespace \
                           WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped \
                           ORDER BY a.attnum";
                let rows = self.lookup(sql, &[schema.as_str(), rel.as_str()]).await?;
                Ok(rows
                    .iter()
                    .map(|r| {
                        let name = col(r, 0);
                        let ty = col(r, 1);
                        let not_null = col(r, 2) == "true";
                        let pk = col(r, 3) == "true";
                        let mut detail = ty;
                        if pk {
                            detail.push_str(" · PK");
                        } else if not_null {
                            detail.push_str(" · not null");
                        }
                        let mut p = path.to_vec();
                        p.push(name.clone());
                        let mut n = node(name.clone(), p, "column", Some(detail), false, None);
                        n.insert_text = Some(quote_ident(&name));
                        n
                    })
                    .collect())
            }
            _ => Ok(Vec::new()),
        }
    }

    async fn query(&self, sql: &str, row_limit: usize) -> Result<QueryOutput> {
        let started = Instant::now();
        if self.read_only_client_check && !risk::is_read_only(sql) {
            bail!("this connection is read-only; only queries that read data can run");
        }
        self.notices.lock().clear();
        let types = self.column_types(sql).await;
        let tx_before = self.tx_state().await.unwrap_or(TxState::Idle);

        // A single plain query is read through a cursor, so the server stops
        // after `row_limit` rows instead of sending the whole result.
        let cursor = if tx_before == TxState::Failed { None } else { risk::as_cursor_query(sql) };
        let results = match cursor {
            Some(q) => {
                let own_tx = tx_before == TxState::Idle;
                let prefix = format!("{}DECLARE shellport_cursor NO SCROLL CURSOR FOR ", if own_tx { "BEGIN; " } else { "" });
                let script = format!(
                    "{prefix}{q}; FETCH {} FROM shellport_cursor; CLOSE shellport_cursor;{}",
                    row_limit + 1,
                    if own_tx { " COMMIT;" } else { "" },
                );
                // Error positions must point into the user's text, not the wrapper.
                let offset = prefix.chars().count() as u32;
                match self.run_script(&script, &types, row_limit).await.map_err(|e| shift_position(e, offset)) {
                    // Keep only the FETCH result (it is the one with columns).
                    Ok(rs) => {
                        let mut fetched = rs.into_iter().find(|r| !r.columns.is_empty()).unwrap_or_default();
                        fetched.rows_affected = Some(fetched.rows.len() as u64);
                        vec![fetched]
                    }
                    Err(e) => {
                        if own_tx {
                            let _ = self.client.simple_query("ROLLBACK").await;
                        }
                        return Err(e);
                    }
                }
            }
            None => self.run_script(sql, &types, row_limit).await?,
        };
        let tx = self.tx_state().await.unwrap_or(TxState::Idle);
        let notices = std::mem::take(&mut *self.notices.lock());
        Ok(QueryOutput { results, notices, elapsed_ms: started.elapsed().as_millis() as u64, tx })
    }

    async fn cancel(&self) -> Result<()> {
        self.client.cancel_token().cancel_query(self.tls.clone()).await.map_err(pg_err)
    }

    async fn close(&self) {
        self.conn_task.abort();
    }

    fn is_closed(&self) -> bool {
        self.client.is_closed()
    }

    async fn tx_state(&self) -> Result<TxState> {
        // Inside a transaction block now() is the block's start time, so it
        // differs from the time this statement started; outside, they match.
        match self.client.simple_query("SELECT now() = statement_timestamp()").await {
            Ok(msgs) => {
                let same = msgs.iter().find_map(|m| match m {
                    SimpleQueryMessage::Row(r) => r.get(0).map(|v| v == "t"),
                    _ => None,
                });
                Ok(if same.unwrap_or(true) { TxState::Idle } else { TxState::Open })
            }
            Err(e) if e.as_db_error().is_some_and(|d| d.code() == &tokio_postgres::error::SqlState::IN_FAILED_SQL_TRANSACTION) => Ok(TxState::Failed),
            Err(e) => Err(pg_err(e)),
        }
    }
}

impl PgSession {
    /// Run a script over the simple protocol, keeping at most `row_limit` rows
    /// per statement. `types` (if known) apply to every result's columns.
    async fn run_script(&self, sql: &str, types: &[Type], row_limit: usize) -> Result<Vec<StatementResult>> {
        let stream = self.client.simple_query_raw(sql).await.map_err(pg_err)?;
        pin_mut!(stream);
        let mut results = Vec::new();
        let mut cur: Option<StatementResult> = None;
        while let Some(msg) = stream.next().await {
            match msg.map_err(pg_err)? {
                SimpleQueryMessage::RowDescription(cols) => {
                    cur = Some(StatementResult {
                        columns: cols
                            .iter()
                            .enumerate()
                            .map(|(i, c)| ColumnMeta { name: c.name().to_string(), type_name: types.get(i).map(|t| t.name().to_string()).unwrap_or_default() })
                            .collect(),
                        ..Default::default()
                    });
                }
                SimpleQueryMessage::Row(row) => {
                    let r = cur.get_or_insert_with(StatementResult::default);
                    if r.rows.len() >= row_limit {
                        r.truncated = true;
                        continue;
                    }
                    r.rows.push((0..row.len()).map(|i| text_to_cell(row.get(i), types.get(i))).collect());
                }
                SimpleQueryMessage::CommandComplete(n) => {
                    let mut r = cur.take().unwrap_or_default();
                    r.rows_affected = Some(n);
                    results.push(r);
                }
                _ => {}
            }
        }
        if let Some(r) = cur.take() {
            results.push(r);
        }
        Ok(results)
    }
}

/// Text of column `i` of a lookup row ("" for NULL).
fn col(r: &[Option<String>], i: usize) -> String {
    r.get(i).cloned().flatten().unwrap_or_default()
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// Map a text-protocol value to a typed cell when the column type is known.
fn text_to_cell(v: Option<&str>, ty: Option<&Type>) -> Cell {
    let Some(v) = v else { return Cell::Null };
    let Some(ty) = ty else { return Cell::Text(v.to_string()) };
    match *ty {
        Type::BOOL => Cell::Bool(v == "t"),
        Type::INT2 | Type::INT4 | Type::INT8 | Type::OID => v.parse().map(Cell::Int).unwrap_or_else(|_| Cell::Text(v.into())),
        Type::FLOAT4 | Type::FLOAT8 => match v.parse::<f64>() {
            Ok(f) if f.is_finite() => Cell::Float(f),
            _ => Cell::Text(v.into()),
        },
        Type::JSON | Type::JSONB => Cell::Json(v.into()),
        Type::BYTEA => Cell::Bytes(v.into()),
        _ => Cell::Text(v.into()),
    }
}

/// Move "(at character N)" back by `offset` characters.
fn shift_position(e: anyhow::Error, offset: u32) -> anyhow::Error {
    let msg = format!("{e:#}");
    let Some(start) = msg.rfind("(at character ") else { return e };
    let rest = &msg[start + 14..];
    let Some(end) = rest.find(')') else { return e };
    match rest[..end].parse::<u32>() {
        Ok(p) if p > offset => anyhow!("{}(at character {}){}", &msg[..start], p - offset, &rest[end + 1..]),
        _ => e,
    }
}

/// Server errors with their detail and hint, like psql prints them.
fn pg_err(e: tokio_postgres::Error) -> anyhow::Error {
    match e.as_db_error() {
        Some(db) => {
            let mut s = format!("{}: {}", db.severity(), db.message());
            if let Some(d) = db.detail() {
                s.push_str(&format!("\nDETAIL: {d}"));
            }
            if let Some(h) = db.hint() {
                s.push_str(&format!("\nHINT: {h}"));
            }
            if let Some(tokio_postgres::error::ErrorPosition::Original(p)) = db.position() {
                s.push_str(&format!(" (at character {p})"));
            }
            anyhow!(s)
        }
        None => {
            // Error's Display hides the cause ("error connecting to server").
            let mut s = e.to_string();
            let mut src = std::error::Error::source(&e);
            while let Some(c) = src {
                s.push_str(&format!(": {c}"));
                src = c.source();
            }
            anyhow!(s)
        }
    }
}

fn tls_connector(verify: bool) -> Result<MakeRustlsConnect> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(provider.clone()).with_safe_default_protocol_versions()?;
    let config = if verify {
        let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
        builder.with_root_certificates(roots).with_no_client_auth()
    } else {
        builder.dangerous().with_custom_certificate_verifier(Arc::new(NoVerify(provider))).with_no_client_auth()
    };
    Ok(MakeRustlsConnect::new(config))
}

/// `sslmode=prefer/require`: encrypt, but accept any certificate (as libpq does).
#[derive(Debug)]
struct NoVerify(Arc<CryptoProvider>);

impl ServerCertVerifier for NoVerify {
    fn verify_server_cert(&self, _: &CertificateDer<'_>, _: &[CertificateDer<'_>], _: &ServerName<'_>, _: &[u8], _: UnixTime) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(m, c, d, &self.0.signature_verification_algorithms)
    }
    fn verify_tls13_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(m, c, d, &self.0.signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn typed_cells_from_text() {
        assert_eq!(text_to_cell(None, Some(&Type::INT4)), Cell::Null);
        assert_eq!(text_to_cell(Some("42"), Some(&Type::INT8)), Cell::Int(42));
        assert_eq!(text_to_cell(Some("t"), Some(&Type::BOOL)), Cell::Bool(true));
        assert_eq!(text_to_cell(Some("1.5"), Some(&Type::FLOAT8)), Cell::Float(1.5));
        assert_eq!(text_to_cell(Some("NaN"), Some(&Type::FLOAT8)), Cell::Text("NaN".into()));
        assert_eq!(text_to_cell(Some("{\"a\":1}"), Some(&Type::JSONB)), Cell::Json("{\"a\":1}".into()));
        assert_eq!(text_to_cell(Some("12.50"), Some(&Type::NUMERIC)), Cell::Text("12.50".into()));
        assert_eq!(text_to_cell(Some("7"), None), Cell::Text("7".into()));
    }

    #[test]
    fn error_positions_shift_back() {
        let e = shift_position(anyhow!("ERROR: syntax error at or near \"7\" (at character 89)"), 53);
        assert_eq!(e.to_string(), "ERROR: syntax error at or near \"7\" (at character 36)");
        assert_eq!(shift_position(anyhow!("ERROR: boom"), 53).to_string(), "ERROR: boom");
    }

    #[test]
    fn tls_connectors_build() {
        tls_connector(true).unwrap();
        tls_connector(false).unwrap();
    }

    fn params(options: &[(&str, &str)], read_only: bool) -> ConnectParams {
        ConnectParams {
            host: "127.0.0.1".into(),
            connect_host: "127.0.0.1".into(),
            connect_port: 55432,
            database: "shellport".into(),
            username: "shellport".into(),
            password: Some("shellport-test".to_string().into()),
            ssl_mode: SslMode::Prefer,
            options: options.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<BTreeMap<_, _>>(),
            read_only,
        }
    }

    /// Needs the PgBouncer from dev/docker-compose.test.yml (transaction mode,
    /// no prepared statements): `cargo test --lib -- --ignored pgbouncer_live`
    #[tokio::test]
    #[ignore]
    async fn pgbouncer_live() {
        let mut p = params(&[], false);
        p.connect_port = 56432;
        let s = Postgres.connect(p).await.unwrap();
        // Several rounds so statements land on different server connections.
        for _ in 0..5 {
            let out = s.query("SELECT id, total FROM shop.orders ORDER BY id LIMIT 3", 10).await.unwrap();
            assert_eq!(out.results[0].rows.len(), 3);
            assert!(s.children(&["shop".into(), "tables".into()]).await.unwrap().iter().any(|n| n.name == "orders"));
            assert!(!s.children(&["shop".into(), "tables".into(), "orders".into()]).await.unwrap().is_empty());
        }
        // Read-only without the pooler flag: falls back to pooler mode.
        let mut p = params(&[], true);
        p.connect_port = 56432;
        let ro = Postgres.connect(p).await.unwrap();
        assert!(ro.query("SELECT 1", 10).await.is_ok());
        assert!(ro.query("DELETE FROM shop.orders WHERE id = -1", 10).await.unwrap_err().to_string().contains("read-only"));
    }

    /// Needs the Postgres from dev/docker-compose.test.yml:
    /// `cargo test --lib -- --ignored postgres_live`
    #[tokio::test]
    #[ignore]
    async fn postgres_live() {
        for pooler in ["0", "1"] {
            let s = Postgres.connect(params(&[("pooler", pooler)], false)).await.unwrap();
            assert!(s.server_version().starts_with("PostgreSQL"));
            let out = s.query("SELECT 1 AS one, 'x'::text AS t, NULL::int AS n", 10).await.unwrap();
            let expect_one = if pooler == "0" { Cell::Int(1) } else { Cell::Text("1".into()) };
            assert_eq!(out.results[0].rows[0], vec![expect_one, Cell::Text("x".into()), Cell::Null], "pooler={pooler}");
            let out = s.query("CREATE TEMP TABLE t(i int); INSERT INTO t SELECT generate_series(1,5); SELECT * FROM t", 3).await.unwrap();
            assert_eq!(out.results.len(), 3);
            assert!(out.results[2].truncated);
            assert_eq!(out.results[2].rows.len(), 3);
            let schemas = s.children(&[]).await.unwrap();
            assert!(schemas.iter().any(|n| n.name == "public"));
            let cols = s.children(&["shop".into(), "tables".into(), "orders".into()]).await.unwrap();
            assert!(cols.iter().any(|c| c.name == "id" && c.detail.as_deref().unwrap_or("").contains("PK")));
        }

        let s = Postgres.connect(params(&[], false)).await.unwrap();
        // Errors on the cursor path point into the user's text.
        let err = s.query("SELECT n, 7 7 FROM generate_series(1, 3) n", 10).await.unwrap_err().to_string();
        assert!(err.ends_with("(at character 13)"), "{err}");
        // Cursor path: the server stops after the limit.
        let t = Instant::now();
        let out = s.query("SELECT generate_series(1, 1000000) AS n", 100).await.unwrap();
        assert_eq!(out.results.len(), 1);
        assert_eq!(out.results[0].rows.len(), 100);
        assert!(out.results[0].truncated);
        assert!(t.elapsed() < Duration::from_secs(1), "{:?}", t.elapsed());
        assert_eq!(out.tx, TxState::Idle);

        // Transaction state, including inside the user's own transaction.
        let out = s.query("BEGIN", 10).await.unwrap();
        assert_eq!(out.tx, TxState::Open);
        let out = s.query("SELECT 1", 10).await.unwrap();
        assert_eq!(out.tx, TxState::Open, "cursor path must not commit the user's transaction");
        assert!(s.query("SELECT 1/0", 10).await.is_err());
        assert_eq!(s.tx_state().await.unwrap(), TxState::Failed);
        let out = s.query("ROLLBACK", 10).await.unwrap();
        assert_eq!(out.tx, TxState::Idle);
        // A failing cursor query outside a transaction leaves none open.
        assert!(s.query("SELECT 1/0", 10).await.is_err());
        assert_eq!(s.tx_state().await.unwrap(), TxState::Idle);

        // Cancel a long query from another task.
        let s2 = s.clone();
        let slow = tokio::spawn(async move { s2.query("SELECT pg_sleep(30)", 1).await });
        tokio::time::sleep(Duration::from_millis(500)).await;
        s.cancel().await.unwrap();
        let err = slow.await.unwrap().unwrap_err().to_string();
        assert!(err.contains("cancel"), "{err}");
        assert_eq!(s.tx_state().await.unwrap(), TxState::Idle);

        // Read-only: enforced by the server, and by us in pooler mode.
        for pooler in ["0", "1"] {
            let ro = Postgres.connect(params(&[("pooler", pooler)], true)).await.unwrap();
            assert!(ro.query("SELECT count(*) FROM shop.orders", 10).await.is_ok());
            let err = ro.query("UPDATE shop.orders SET paid = paid WHERE id = 1", 10).await.unwrap_err().to_string();
            assert!(err.contains("read-only"), "pooler={pooler}: {err}");
        }
    }
}

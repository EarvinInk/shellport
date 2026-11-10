//! PostgreSQL driver (tokio-postgres + rustls/ring).
//!
//! Data goes over the simple-query (text) protocol, which runs any script
//! (several statements, DDL, `BEGIN`…`COMMIT`) the way psql does. For a single
//! statement the query is also prepared, only to learn the column types, so
//! numbers, booleans and JSON come back typed.

use crate::db::{
    quote_ident, Capabilities, Cell, ColumnMeta, ConnectParams, DbDriver, DbEngine, DbSession, EngineInfo, QueryOutput, SchemaNode, SslMode,
    StatementResult,
};
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
        let tls = tls_connector(p.ssl_mode == SslMode::VerifyFull)?;
        let (client, mut conn) = cfg.connect(tls.clone()).await.map_err(pg_err)?;

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
        Ok(Arc::new(PgSession { client, tls, notices, conn_task: task.abort_handle(), version }))
    }
}

struct PgSession {
    client: Client,
    tls: MakeRustlsConnect,
    notices: Arc<Mutex<Vec<String>>>,
    conn_task: AbortHandle,
    version: String,
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
                let rows = self.client.query(&sql, &[]).await.map_err(pg_err)?;
                Ok(rows
                    .iter()
                    .map(|r| {
                        let s: String = r.get(0);
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
                    "SELECT c.relname::text, c.relkind::text, c.reltuples::bigint FROM pg_class c \
                     JOIN pg_namespace n ON n.oid = c.relnamespace \
                     WHERE n.nspname = $1 AND c.relkind IN {kinds} ORDER BY c.relname"
                );
                let rows = self.client.query(&sql, &[schema]).await.map_err(pg_err)?;
                Ok(rows
                    .iter()
                    .map(|r| {
                        let name: String = r.get(0);
                        let kind: String = r.get(1);
                        let est: i64 = r.get(2);
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
                let rows = self.client.query(sql, &[schema]).await.map_err(pg_err)?;
                Ok(rows
                    .iter()
                    .map(|r| {
                        let name: String = r.get(0);
                        let args: String = r.get(1);
                        let kind: String = r.get(2);
                        let mut n = node(format!("{name}({args})"), vec![schema.clone(), folder.clone(), name.clone()], &kind, None, false, None);
                        n.insert_text = Some(format!("{}.{}", quote_ident(schema), quote_ident(&name)));
                        n
                    })
                    .collect())
            }
            [schema, _, rel] => {
                let sql = "SELECT a.attname::text, format_type(a.atttypid, a.atttypmod), a.attnotnull, \
                           EXISTS (SELECT 1 FROM pg_index i WHERE i.indrelid = c.oid AND i.indisprimary AND a.attnum = ANY(i.indkey)) \
                           FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid \
                           JOIN pg_namespace n ON n.oid = c.relnamespace \
                           WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped \
                           ORDER BY a.attnum";
                let rows = self.client.query(sql, &[schema, rel]).await.map_err(pg_err)?;
                Ok(rows
                    .iter()
                    .map(|r| {
                        let name: String = r.get(0);
                        let ty: String = r.get(1);
                        let not_null: bool = r.get(2);
                        let pk: bool = r.get(3);
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
        self.notices.lock().clear();
        // Only a single statement can be prepared; for scripts this fails
        // and every value is shown as text.
        let types: Vec<Type> = match self.client.prepare(sql).await {
            Ok(st) => st.columns().iter().map(|c| c.type_().clone()).collect(),
            Err(_) => Vec::new(),
        };
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
        let notices = std::mem::take(&mut *self.notices.lock());
        Ok(QueryOutput { results, notices, elapsed_ms: started.elapsed().as_millis() as u64 })
    }

    async fn cancel(&self) -> Result<()> {
        self.client.cancel_token().cancel_query(self.tls.clone()).await.map_err(pg_err)
    }

    async fn close(&self) {
        self.conn_task.abort();
    }
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
    fn tls_connectors_build() {
        tls_connector(true).unwrap();
        tls_connector(false).unwrap();
    }

    /// Needs the Postgres from dev/docker-compose.test.yml:
    /// `cargo test --lib -- --ignored postgres_live`
    #[tokio::test]
    #[ignore]
    async fn postgres_live() {
        let s = Postgres
            .connect(ConnectParams {
                host: "127.0.0.1".into(),
                connect_host: "127.0.0.1".into(),
                connect_port: 55432,
                database: "shellport".into(),
                username: "shellport".into(),
                password: Some("shellport-test".to_string().into()),
                ssl_mode: SslMode::Prefer,
                options: BTreeMap::new(),
            })
            .await
            .unwrap();
        assert!(s.server_version().starts_with("PostgreSQL"));
        let out = s.query("SELECT 1 AS one, 'x'::text AS t, NULL::int AS n", 10).await.unwrap();
        assert_eq!(out.results[0].rows[0], vec![Cell::Int(1), Cell::Text("x".into()), Cell::Null]);
        let out = s.query("CREATE TEMP TABLE t(i int); INSERT INTO t SELECT generate_series(1,5); SELECT * FROM t", 3).await.unwrap();
        assert_eq!(out.results.len(), 3);
        assert!(out.results[2].truncated);
        assert_eq!(out.results[2].rows.len(), 3);
        let schemas = s.children(&[]).await.unwrap();
        assert!(schemas.iter().any(|n| n.name == "public"));
        // Cancel a long query from another task.
        let s2 = s.clone();
        let slow = tokio::spawn(async move { s2.query("SELECT pg_sleep(30)", 1).await });
        tokio::time::sleep(Duration::from_millis(500)).await;
        s.cancel().await.unwrap();
        let err = slow.await.unwrap().unwrap_err().to_string();
        assert!(err.contains("cancel"), "{err}");
    }
}

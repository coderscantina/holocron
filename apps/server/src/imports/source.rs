//! Read-only access to an Umami source database.
//!
//! A source off the private network that the URL doesn't read over TLS gets a warning and is
//! read anyway ([`unencrypted`], decision 0021).
//! Every session is read only before the first query, also for credentials that could write: PostgreSQL through
//! `default_transaction_read_only`, MySQL through `SET SESSION TRANSACTION READ ONLY`. Reads of one
//! run share one repeatable-read snapshot. Values cross as text, integers, flags and bytes only:
//! timestamps become Unix milliseconds and decimals keep their exact text, so both engines
//! decode through one row type.

use std::net::IpAddr;
use std::str::FromStr;

use serde_json::{Map, Value};
use sqlx::AnyConnection;
use sqlx::any::AnyRow;
use sqlx::mysql::{MySqlConnectOptions, MySqlSslMode};
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::{Connection, Executor, Row};

use super::signature::{Column, Engine, Migration, Observed};
use crate::init::{Exit, Failure};

pub struct Source {
    /// Set when the connection may travel unencrypted, see [`unencrypted`]. The importer's
    /// `warnings` report and log it.
    pub warning: Option<String>,
    /// Only ever reached through `&mut self` with `get_mut`, which takes no lock. The mutex
    /// makes `Source` `Sync`, so the setup wizard can run an import as a task (`AnyConnection`
    /// is `Send` but not `Sync`).
    conn: std::sync::Mutex<AnyConnection>,
    pub engine: Engine,
}

/// How a column crosses from the source.
#[derive(Debug, Clone, Copy)]
pub enum Kind {
    Text,
    /// A timestamp, as Unix milliseconds.
    Ms,
    Int,
    Flag,
    Bytes,
}

/// A source row by column name, in the JSON form inventories preserve.
pub type SourceRow = Map<String, Value>;

/// A bound query argument.
pub enum Arg<'a> {
    Text(&'a str),
    Int(i64),
}

pub fn unavailable(error: impl std::fmt::Display) -> Failure {
    Failure::new(
        Exit::StorageUnavailable,
        "source_unavailable",
        error.to_string(),
    )
}

/// Warns of a connection that could travel unencrypted to a host off the private network: the
/// URL doesn't ask for TLS (`sslmode=require` or stricter on PostgreSQL, `ssl-mode=REQUIRED` or
/// stricter on MySQL) and the host is not a socket, loopback, a private or link-local address, or
/// a single-label name such as a Docker service. Names are judged as written, not resolved, so
/// `host.docker.internal` or a VPC-internal DNS name gets the warning too. Only a URL that
/// doesn't parse fails.
pub fn unencrypted(url: &str, engine: Engine) -> Result<Option<String>, Failure> {
    let usage = |e: sqlx::Error| Failure::new(Exit::Usage, "usage", format!("--source: {e}"));
    let (host, local, tls) = match engine {
        Engine::Postgresql => {
            let options = PgConnectOptions::from_str(url).map_err(usage)?;
            let tls = matches!(
                options.get_ssl_mode(),
                PgSslMode::Require | PgSslMode::VerifyCa | PgSslMode::VerifyFull
            );
            let local = options.get_socket().is_some() || options.get_host().starts_with('/');
            (options.get_host().to_owned(), local, tls)
        }
        Engine::Mysql => {
            let options = MySqlConnectOptions::from_str(url).map_err(usage)?;
            let tls = matches!(
                options.get_ssl_mode(),
                MySqlSslMode::Required | MySqlSslMode::VerifyCa | MySqlSslMode::VerifyIdentity
            );
            (
                options.get_host().to_owned(),
                options.get_socket().is_some(),
                tls,
            )
        }
    };
    if tls || local || private_host(&host) {
        return Ok(None);
    }
    let (parameter, modes) = match engine {
        Engine::Postgresql => (
            "sslmode",
            "verify-full, or require to skip the certificate check",
        ),
        Engine::Mysql => (
            "ssl-mode",
            "VERIFY_IDENTITY, or REQUIRED to skip the certificate check",
        ),
    };
    Ok(Some(format!(
        "{host} is not on a private network and the URL doesn't require TLS, so the credentials and the Umami data may cross the network unencrypted. Holocron reads it anyway. To require TLS, add `{parameter}` to the URL: {modes}. A certificate from a private CA, such as Amazon RDS's, needs its CA file as `sslrootcert` (PostgreSQL) or `ssl-ca` (MySQL)."
    )))
}

/// Loopback, private and link-local addresses, `localhost`, and names without a dot.
fn private_host(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        Ok(IpAddr::V6(v6)) => match v6.to_ipv4_mapped() {
            Some(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
            None => v6.is_loopback() || v6.is_unique_local() || v6.is_unicast_link_local(),
        },
        Err(_) => {
            let name = host.trim_end_matches('.').to_ascii_lowercase();
            !name.is_empty()
                && (name == "localhost" || name.ends_with(".localhost") || !name.contains('.'))
        }
    }
}

impl Source {
    /// Connects to `postgres://`, `postgresql://` or `mysql://` and makes the session read only.
    pub async fn connect(url: &str) -> Result<Self, Failure> {
        let engine = match url.split_once("://").map(|(scheme, _)| scheme) {
            Some("postgres" | "postgresql") => Engine::Postgresql,
            Some("mysql") => Engine::Mysql,
            _ => {
                return Err(Failure::new(
                    Exit::Usage,
                    "usage",
                    "--source must be a postgres:// or mysql:// URL",
                ));
            }
        };
        let warning = unencrypted(url, engine)?;
        sqlx::any::install_default_drivers();
        let mut conn = AnyConnection::connect(url).await.map_err(unavailable)?;
        let setup: &[&str] = match engine {
            Engine::Postgresql => &[
                "SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY",
                "SET TIME ZONE 'UTC'",
            ],
            Engine::Mysql => &[
                "SET SESSION TRANSACTION READ ONLY",
                "SET SESSION TRANSACTION ISOLATION LEVEL REPEATABLE READ",
                "SET SESSION time_zone = '+00:00'",
            ],
        };
        for statement in setup {
            conn.execute(*statement).await.map_err(unavailable)?;
        }
        Ok(Self {
            warning,
            conn: std::sync::Mutex::new(conn),
            engine,
        })
    }

    /// Starts the repeatable-read snapshot every later read of this connection sees.
    pub async fn begin_snapshot(&mut self) -> Result<(), Failure> {
        let statement = match self.engine {
            Engine::Postgresql => "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY",
            Engine::Mysql => "START TRANSACTION WITH CONSISTENT SNAPSHOT, READ ONLY",
        };
        self.conn()
            .execute(statement)
            .await
            .map(drop)
            .map_err(unavailable)
    }

    pub async fn close(self) {
        if let Ok(conn) = self.conn.into_inner() {
            let _ = conn.close().await;
        }
    }

    fn conn(&mut self) -> &mut AnyConnection {
        self.conn
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Whether the database has a `_prisma_migrations` table at all.
    pub async fn has_migration_table(&mut self) -> Result<bool, Failure> {
        let sql = match self.engine {
            Engine::Postgresql => {
                "SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
                 WHERE n.nspname = current_schema() AND c.relname = '_prisma_migrations'"
            }
            Engine::Mysql => {
                "SELECT count(*) FROM information_schema.tables
                 WHERE table_schema = DATABASE() AND table_name = '_prisma_migrations'"
            }
        };
        Ok(self.scalar(sql, &[]).await? > 0)
    }

    /// The migration table and column catalog inspection compares.
    pub async fn observe(&mut self) -> Result<Observed, Failure> {
        let flag = |expr: &str| self.engine.flag(expr);
        let migrations = format!(
            "SELECT {}, {}, {}, {} FROM _prisma_migrations",
            self.engine.text("migration_name"),
            self.engine.text("checksum"),
            flag("finished_at IS NOT NULL"),
            flag("rolled_back_at IS NOT NULL"),
        );
        let migrations = self
            .rows(&migrations, &[])
            .await?
            .iter()
            .map(|r| {
                Ok(Migration {
                    name: r.try_get(0)?,
                    checksum: r.try_get(1)?,
                    finished: r.try_get::<i64, _>(2)? == 1,
                    rolled_back: r.try_get::<i64, _>(3)? == 1,
                })
            })
            .collect::<Result<_, sqlx::Error>>()
            .map_err(unavailable)?;
        let columns = match self.engine {
            Engine::Postgresql => {
                "SELECT c.relname::text, a.attname::text, format_type(a.atttypid, a.atttypmod),
                        (NOT a.attnotnull)::int
                 FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid
                 JOIN pg_namespace n ON n.oid = c.relnamespace
                 WHERE n.nspname = current_schema() AND c.relkind = 'r' AND a.attnum > 0
                   AND NOT a.attisdropped AND c.relname <> '_prisma_migrations'"
            }
            Engine::Mysql => {
                "SELECT CAST(table_name AS CHAR), CAST(column_name AS CHAR), CAST(column_type AS CHAR),
                        CAST(is_nullable = 'YES' AS SIGNED)
                 FROM information_schema.columns
                 WHERE table_schema = DATABASE() AND table_name <> '_prisma_migrations'"
            }
        };
        let columns = self
            .rows(columns, &[])
            .await?
            .iter()
            .map(|r| {
                Ok(Column {
                    table: r.try_get(0)?,
                    column: r.try_get(1)?,
                    data_type: r.try_get(2)?,
                    nullable: r.try_get::<i64, _>(3)? == 1,
                })
            })
            .collect::<Result<_, sqlx::Error>>()
            .map_err(unavailable)?;
        Ok(Observed {
            engine: self.engine,
            migrations,
            columns,
        })
    }

    /// Tables or privileges that let these credentials change the source. Empty for a read-only
    /// role, which is what the importer requires.
    pub async fn write_grants(&mut self) -> Result<Vec<String>, Failure> {
        let sql = match self.engine {
            Engine::Postgresql => {
                "SELECT c.relname::text FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
                 WHERE n.nspname = current_schema() AND c.relkind = 'r'
                   AND (has_table_privilege(c.oid, 'INSERT') OR has_table_privilege(c.oid, 'UPDATE')
                        OR has_table_privilege(c.oid, 'DELETE') OR has_table_privilege(c.oid, 'TRUNCATE'))
                 ORDER BY 1"
            }
            Engine::Mysql => {
                "SELECT DISTINCT CAST(privilege_type AS CHAR) FROM (
                     SELECT privilege_type, grantee FROM information_schema.user_privileges
                     UNION ALL SELECT privilege_type, grantee FROM information_schema.schema_privileges
                         WHERE table_schema = DATABASE()
                     UNION ALL SELECT privilege_type, grantee FROM information_schema.table_privileges
                         WHERE table_schema = DATABASE()) p
                 WHERE grantee = CONCAT('''', SUBSTRING_INDEX(CURRENT_USER(), '@', 1), '''@''',
                                        SUBSTRING_INDEX(CURRENT_USER(), '@', -1), '''')
                   AND privilege_type IN ('INSERT', 'UPDATE', 'DELETE', 'DROP', 'ALTER', 'CREATE',
                                          'INDEX', 'TRIGGER', 'SUPER')
                 ORDER BY 1"
            }
        };
        self.rows(sql, &[])
            .await?
            .iter()
            .map(|r| r.try_get(0))
            .collect::<Result<_, _>>()
            .map_err(unavailable)
    }

    /// Rows of `table` with `columns`, after an optional SQL condition, in `order`.
    pub async fn table(
        &mut self,
        table: &str,
        columns: &[(&str, Kind)],
        condition: &str,
        order: &str,
        args: &[Arg<'_>],
    ) -> Result<Vec<SourceRow>, Failure> {
        let select: Vec<String> = columns
            .iter()
            .map(|(name, kind)| self.engine.column(name, *kind))
            .collect();
        let mut sql = format!(
            "SELECT {} FROM {}",
            select.join(", "),
            self.engine.quote(table)
        );
        if !condition.is_empty() {
            sql = format!("{sql} WHERE {condition}");
        }
        if !order.is_empty() {
            sql = format!("{sql} ORDER BY {order}");
        }
        let rows = self.rows(&sql, args).await?;
        rows.iter()
            .map(|row| decode(row, columns))
            .collect::<Result<_, _>>()
            .map_err(unavailable)
    }

    /// Runs `sql` with `?` placeholders, numbered for PostgreSQL.
    pub async fn rows(&mut self, sql: &str, args: &[Arg<'_>]) -> Result<Vec<AnyRow>, Failure> {
        let sql = self.engine.placeholders(sql);
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
        for arg in args {
            query = match arg {
                Arg::Text(text) => query.bind(*text),
                Arg::Int(value) => query.bind(*value),
            };
        }
        query.fetch_all(self.conn()).await.map_err(unavailable)
    }

    pub async fn scalar(&mut self, sql: &str, args: &[Arg<'_>]) -> Result<i64, Failure> {
        let rows = self.rows(sql, args).await?;
        rows.first()
            .map_or(Ok(0), |r| {
                r.try_get::<Option<i64>, _>(0)
                    .map(Option::unwrap_or_default)
            })
            .map_err(unavailable)
    }
}

/// A row of `columns` in the JSON form inventories keep.
pub fn decode(row: &AnyRow, columns: &[(&str, Kind)]) -> Result<SourceRow, sqlx::Error> {
    let mut out = Map::new();
    for (i, (name, kind)) in columns.iter().enumerate() {
        let value = match kind {
            Kind::Text => row.try_get::<Option<String>, _>(i)?.map(Value::from),
            Kind::Ms | Kind::Int => row.try_get::<Option<i64>, _>(i)?.map(Value::from),
            Kind::Flag => row
                .try_get::<Option<i64>, _>(i)?
                .map(|v| Value::from(v != 0)),
            Kind::Bytes => row
                .try_get::<Option<Vec<u8>>, _>(i)?
                .map(|bytes| Value::from(data_encoding::BASE64.encode(&bytes))),
        };
        out.insert((*name).to_owned(), value.unwrap_or(Value::Null));
    }
    Ok(out)
}

impl Engine {
    /// A table name, quoted (`user` is reserved in both engines).
    pub fn quote(self, name: &str) -> String {
        match self {
            Engine::Postgresql => format!("\"{name}\""),
            Engine::Mysql => format!("`{name}`"),
        }
    }

    /// A column read as `kind`.
    pub fn column(self, name: &str, kind: Kind) -> String {
        let quoted = self.quote(name);
        match kind {
            Kind::Text => self.text(&quoted),
            Kind::Ms => self.ms(&quoted),
            Kind::Int => self.int(&quoted),
            Kind::Flag => self.flag(&quoted),
            Kind::Bytes => quoted,
        }
    }

    pub fn text(self, expr: &str) -> String {
        match self {
            Engine::Postgresql => format!("({expr})::text"),
            Engine::Mysql => format!("CAST({expr} AS CHAR)"),
        }
    }

    /// Unix milliseconds of a timestamp, floored.
    pub fn ms(self, expr: &str) -> String {
        match self {
            Engine::Postgresql => format!("floor(extract(epoch from {expr}) * 1000)::bigint"),
            Engine::Mysql => format!("CAST(FLOOR(UNIX_TIMESTAMP({expr}) * 1000) AS SIGNED)"),
        }
    }

    pub fn int(self, expr: &str) -> String {
        match self {
            Engine::Postgresql => format!("({expr})::bigint"),
            Engine::Mysql => format!("CAST({expr} AS SIGNED)"),
        }
    }

    /// A boolean expression as 0 or 1.
    pub fn flag(self, expr: &str) -> String {
        match self {
            Engine::Postgresql => format!("({expr})::int"),
            Engine::Mysql => format!("CAST(({expr}) AS SIGNED)"),
        }
    }

    /// A UUID parameter: PostgreSQL compares `uuid` columns, MySQL `varchar(36)`.
    pub fn id_param(self) -> &'static str {
        match self {
            Engine::Postgresql => "?::uuid",
            Engine::Mysql => "?",
        }
    }

    /// An instant parameter in Unix milliseconds, compared with a timestamp column.
    pub fn ms_param(self) -> &'static str {
        match self {
            Engine::Postgresql => "to_timestamp(?::bigint / 1000.0)",
            Engine::Mysql => "FROM_UNIXTIME(? / 1000)",
        }
    }

    fn placeholders(self, sql: &str) -> String {
        match self {
            Engine::Mysql => sql.to_owned(),
            Engine::Postgresql => {
                let mut out = String::with_capacity(sql.len() + 8);
                let mut n = 0;
                for c in sql.chars() {
                    if c == '?' {
                        n += 1;
                        out.push_str(&format!("${n}"));
                    } else {
                        out.push(c);
                    }
                }
                out
            }
        }
    }
}

/// Getters for the JSON form of a source row.
pub trait RowExt {
    fn text(&self, column: &str) -> Option<&str>;
    fn int(&self, column: &str) -> Option<i64>;
    fn flag(&self, column: &str) -> bool;
}

impl RowExt for SourceRow {
    fn text(&self, column: &str) -> Option<&str> {
        self.get(column).and_then(Value::as_str)
    }

    fn int(&self, column: &str) -> Option<i64> {
        self.get(column).and_then(Value::as_i64)
    }

    fn flag(&self, column: &str) -> bool {
        self.get(column).and_then(Value::as_bool).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn warned(url: &str) -> bool {
        let engine = if url.starts_with("mysql") {
            Engine::Mysql
        } else {
            Engine::Postgresql
        };
        unencrypted(url, engine).unwrap().is_some()
    }

    #[test]
    fn a_public_source_without_tls_gets_a_warning_and_a_private_one_does_not() {
        for url in [
            "postgres://ro:pw@umami.cxyz.eu-central-1.rds.amazonaws.com/umami",
            "postgres://ro:pw@203.0.113.7/umami?sslmode=prefer",
            "postgres://ro:pw@host.docker.internal/umami",
            "mysql://ro:pw@db.example.com/umami?ssl-mode=PREFERRED",
            "mysql://ro:pw@[2001:db8::1]:3306/umami",
        ] {
            assert!(warned(url), "{url}");
        }
        for url in [
            "postgres://ro:pw@umami.cxyz.eu-central-1.rds.amazonaws.com/umami?sslmode=require",
            "postgres://ro:pw@203.0.113.7/umami?sslmode=verify-full",
            "mysql://ro:pw@db.example.com/umami?ssl-mode=VERIFY_IDENTITY",
            "mysql://ro:pw@db.example.com/umami?ssl-mode=required",
            "postgres://ro:pw@127.0.0.1:5432/umami",
            "postgres://ro:pw@localhost/umami?sslmode=disable",
            "postgres://ro:pw@10.0.4.2/umami",
            "mysql://ro:pw@192.168.1.20/umami",
            "mysql://ro:pw@[::1]/umami",
            "mysql://ro:pw@[fd00::5]/umami",
            "postgres://ro:pw@umami-db/umami",
            "postgres:///umami?host=/var/run/postgresql",
        ] {
            assert!(!warned(url), "{url}");
        }
    }
}

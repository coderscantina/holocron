//! Event retention per source (decision 0016): a site's own setting or the installation
//! default. Links and pixels follow the default unless an import set their value. Collection,
//! queries and the hourly job read the effective value here, so one setting decides what is
//! stored, shown and removed.

use chrono_tz::Tz;
use holocron_analytics::identity::{SessionPolicy, SitePolicies};
use holocron_analytics::jobs;
use holocron_protocol::access::InstallRole;
use holocron_protocol::envelope::{Empty, Topic};
use holocron_protocol::operations::retention::{
    RetentionDefaultArgs, RetentionHandlers, RetentionSettings,
};
use holocron_protocol::operations::sites::RetentionPreview;
use holocron_protocol::registry::Snapshot;
use holocron_protocol::retention::Retention;
use holocron_storage::Lane;
use serde_json::json;
use sqlx::{Executor, Sqlite, SqliteConnection};

use crate::context::{CommandCtx, QueryCtx};
use crate::error::ApiError;
use crate::sites::policy::{analytics_id, session_policies};
use crate::state::AppState;

/// The control table of a kind of analytical source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Site,
    Link,
    Pixel,
}

impl Kind {
    pub(crate) fn table(self) -> &'static str {
        match self {
            Kind::Site => "sites",
            Kind::Link => "links",
            Kind::Pixel => "pixels",
        }
    }
}

/// A source with its retention setting and the policies its cutoff is computed in.
pub struct Source {
    pub id: String,
    pub kind: Kind,
    /// Its own setting; `None` follows the installation default.
    pub own: Option<Retention>,
    /// Session policies for sites, UTC for links and pixels (0006).
    pub policies: SitePolicies,
}

impl Source {
    pub fn retention(&self, default: Retention) -> Retention {
        self.own.unwrap_or(default)
    }
}

pub fn parse(code: &str) -> Result<Retention, sqlx::Error> {
    Retention::from_code(code)
        .ok_or_else(|| sqlx::Error::Decode(format!("unknown retention {code}").into()))
}

pub async fn installation_default<'e>(
    conn: impl Executor<'e, Database = Sqlite>,
) -> Result<Retention, sqlx::Error> {
    let code: String =
        sqlx::query_scalar("SELECT event_retention FROM installation_settings WHERE id = 1")
            .fetch_one(conn)
            .await?;
    parse(&code)
}

/// The effective retention of a source, `None` when it does not exist.
pub async fn effective<'e>(
    conn: impl Executor<'e, Database = Sqlite>,
    kind: Kind,
    id: &str,
) -> Result<Option<Retention>, sqlx::Error> {
    lookup(conn, kind, id, "").await
}

/// [`effective`] for a link or pixel that collects: one that is not deleted.
pub async fn effective_live<'e>(
    conn: impl Executor<'e, Database = Sqlite>,
    kind: Kind,
    id: &str,
) -> Result<Option<Retention>, sqlx::Error> {
    lookup(conn, kind, id, " AND deleted_at IS NULL").await
}

async fn lookup<'e>(
    conn: impl Executor<'e, Database = Sqlite>,
    kind: Kind,
    id: &str,
    condition: &str,
) -> Result<Option<Retention>, sqlx::Error> {
    let code: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT coalesce(event_retention,
             (SELECT event_retention FROM installation_settings WHERE id = 1))
         FROM {} WHERE id = ?{condition}",
        kind.table()
    )))
    .bind(id)
    .fetch_optional(conn)
    .await?;
    code.as_deref().map(parse).transpose()
}

/// The site's own setting, `None` when it follows the default.
pub async fn own(
    conn: &mut SqliteConnection,
    site: &str,
) -> Result<Option<Retention>, sqlx::Error> {
    let code: Option<String> = sqlx::query_scalar("SELECT event_retention FROM sites WHERE id = ?")
        .bind(site)
        .fetch_one(conn)
        .await?;
    code.as_deref().map(parse).transpose()
}

/// Every site with a session policy, then every link and pixel.
pub async fn sources(conn: &mut SqliteConnection) -> Result<Vec<Source>, sqlx::Error> {
    let utc = SitePolicies::new(SessionPolicy::new(Tz::UTC, 30).expect("valid default policy"));
    let mut sources = Vec::new();
    for kind in [Kind::Site, Kind::Link, Kind::Pixel] {
        let rows: Vec<(String, Option<String>)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT id, event_retention FROM {}",
            kind.table()
        )))
        .fetch_all(&mut *conn)
        .await?;
        for (id, code) in rows {
            let policies = match kind {
                Kind::Site => match session_policies(&mut *conn, &id).await? {
                    Some(policies) => policies,
                    None => continue,
                },
                Kind::Link | Kind::Pixel => utc.clone(),
            };
            sources.push(Source {
                own: code.as_deref().map(parse).transpose()?,
                id,
                kind,
                policies,
            });
        }
    }
    Ok(sources)
}

impl AppState {
    /// What moving each of `changes` from its current retention to `next` removes. Lengthening
    /// removes nothing; shortening removes the stored events before the new cutoff.
    pub(crate) async fn retention_preview(
        &self,
        changes: Vec<(Source, Retention)>,
        next: Retention,
    ) -> Result<RetentionPreview, ApiError> {
        let now = self.collector.clock().now_ms();
        let mut cutoff = None;
        let mut shortened = Vec::new();
        let mut sources = 0;
        for (source, current) in &changes {
            if *current == next {
                continue;
            }
            sources += 1;
            let Some(at) = source.policies.retention_cutoff_ms(now, next) else {
                continue;
            };
            cutoff = Some(cutoff.map_or(at, |c: i64| c.min(at)));
            if next < *current {
                shortened.push((analytics_id(&source.id).map_err(ApiError::internal)?, at));
            }
        }
        let events_removed = self
            .collector
            .storage()
            .read(Lane::Report, move |snapshot| {
                shortened.iter().try_fold(0, |sum, (site, cutoff)| {
                    Ok(sum + jobs::events_before(snapshot.connection(), *site, *cutoff)?)
                })
            })
            .await?
            .value;
        Ok(RetentionPreview {
            retention: next,
            cutoff,
            events_removed,
            sources,
        })
    }

    /// The sources that follow the installation default, with it as their current retention.
    async fn following_default(
        conn: &mut SqliteConnection,
    ) -> Result<Vec<(Source, Retention)>, ApiError> {
        let default = installation_default(&mut *conn).await?;
        Ok(sources(conn)
            .await?
            .into_iter()
            .filter(|s| s.own.is_none())
            .map(|s| (s, default))
            .collect())
    }
}

async fn settings(
    conn: &mut SqliteConnection,
    can_manage: bool,
) -> Result<RetentionSettings, ApiError> {
    Ok(RetentionSettings {
        installation_default: installation_default(conn).await?,
        can_manage_installation: can_manage,
    })
}

impl RetentionHandlers for AppState {
    async fn retention_update_default(
        &self,
        ctx: &mut CommandCtx,
        args: RetentionDefaultArgs,
    ) -> Result<RetentionSettings, ApiError> {
        ctx.require_install_admin().await?;
        let current = installation_default(ctx.conn()).await?;
        if args.retention < current {
            ctx.require_fresh_login()?;
        }
        if args.retention != current {
            sqlx::query(
                "UPDATE installation_settings SET event_retention = ?, updated_at = ? WHERE id = 1",
            )
            .bind(args.retention.code())
            .bind(ctx.now())
            .execute(ctx.conn())
            .await?;
            ctx.record(
                &Topic::installation(),
                "retention.default_changed",
                json!({ "from": current, "to": args.retention }),
            )
            .await?;
        }
        settings(ctx.conn(), true).await
    }

    async fn retention_settings(
        &self,
        ctx: &mut QueryCtx,
        _args: Empty,
    ) -> Result<Snapshot<RetentionSettings>, ApiError> {
        let user = ctx.person()?;
        let role: String = sqlx::query_scalar("SELECT install_role FROM users WHERE id = ?")
            .bind(user.as_str())
            .fetch_one(ctx.conn())
            .await?;
        let can_manage = InstallRole::parse(&role) == Some(InstallRole::InstallAdmin);
        Ok(Snapshot {
            topic: Topic::installation(),
            data: settings(ctx.conn(), can_manage).await?,
        })
    }

    async fn retention_preview_default(
        &self,
        ctx: &mut QueryCtx,
        args: RetentionDefaultArgs,
    ) -> Result<Snapshot<RetentionPreview>, ApiError> {
        ctx.require_install_admin().await?;
        let changes = Self::following_default(ctx.conn()).await?;
        Ok(Snapshot {
            topic: Topic::installation(),
            data: self.retention_preview(changes, args.retention).await?,
        })
    }
}

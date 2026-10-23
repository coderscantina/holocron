//! Saved goals of a site: `goals.save`, `goals.archive`, `goals.list` and `goals.get`.
//!
//! A goal's definition is versioned (0003 Goals): a save that changes the definition adds the
//! next version, and stored versions never change, so a report holding `ref {id, version}`
//! can always read the version it copied.

use holocron_analytics::explore;
use holocron_protocol::access::Capability;
use holocron_protocol::envelope::Topic;
use holocron_protocol::errors::FieldErrorCode;
use holocron_protocol::exploration::{
    Compare, ExplorationState, GoalDefinition, GoalsReport, Granularity, Range, RangeUnit,
    ReportDefinition, Visualization,
};
use holocron_protocol::ids::{GoalId, SiteId};
use holocron_protocol::operations::goals::{
    GoalsGetArgs, GoalsHandlers, GoalsListArgs, GoalsSaveArgs, SavedGoal, SavedGoalList,
    SavedGoalResult,
};
use holocron_protocol::registry::Snapshot;
use serde_json::json;
use sqlx::SqliteConnection;

use super::{definition_name, refused};
use crate::context::{CommandCtx, QueryCtx};
use crate::error::{ApiError, FieldErrors};
use crate::state::AppState;

/// id, name, version, latest version, definition, archived, version created_at.
type GoalRow = (String, String, i64, i64, String, bool, i64);

fn saved(site: &SiteId, row: GoalRow) -> Result<SavedGoal, ApiError> {
    let (id, name, version, latest, definition, archived, created_at) = row;
    Ok(SavedGoal {
        id: GoalId::new(id),
        site: site.clone(),
        name,
        version: version as u32,
        latest_version: latest as u32,
        definition: serde_json::from_str(&definition).map_err(ApiError::internal)?,
        archived,
        created_at,
    })
}

/// One version of a goal of `site`, the latest without `version`; anything else is not found.
async fn goal(
    conn: &mut SqliteConnection,
    site: &SiteId,
    id: &GoalId,
    version: Option<u32>,
) -> Result<SavedGoal, ApiError> {
    let row: Option<GoalRow> = sqlx::query_as(
        "SELECT g.id, g.name, v.version, g.version, v.definition, g.archived_at IS NOT NULL, v.created_at
         FROM goals g JOIN goal_versions v ON v.goal_id = g.id
         WHERE g.id = ? AND g.site_id = ? AND v.version = coalesce(?, g.version)",
    )
    .bind(id.as_str())
    .bind(site.as_str())
    .bind(version.map(i64::from))
    .fetch_optional(conn)
    .await?;
    saved(site, row.ok_or_else(ApiError::not_found)?)
}

/// A goal passes the planner's checks of a goals report, with refusals named after the
/// definition: `definition.predicate.<id>`.
fn check(definition: &GoalDefinition, site: &SiteId) -> Result<(), ApiError> {
    let state = ExplorationState {
        v: holocron_protocol::exploration::STATE_VERSION,
        site: site.clone(),
        measures: Vec::new(),
        dimensions: Vec::new(),
        filter: None,
        range: Range::Relative {
            unit: RangeUnit::Day,
            count: 1,
        },
        timezone: "UTC".into(),
        granularity: Granularity::Day,
        compare: Compare::None,
        metric_version: holocron_analytics::METRIC_VERSION,
        sort: Vec::new(),
        page: explore::DEFAULT_PAGE,
        visualization: Visualization::Table,
        report: Some(ReportDefinition::Goals(GoalsReport {
            v: holocron_protocol::exploration::REPORT_VERSION,
            goals: vec![definition.clone()],
        })),
    };
    explore::validate(&state, &[])
        .map(drop)
        .map_err(|mut refusal| {
            refusal.field = refusal.field.replacen("report.goals.0.", "definition.", 1);
            refused(refusal)
        })
}

/// Whether an active goal of the site other than `except` already has this name.
async fn name_taken(
    conn: &mut SqliteConnection,
    site: &SiteId,
    key: &str,
    except: Option<&GoalId>,
) -> Result<bool, ApiError> {
    let taken: Option<String> = sqlx::query_scalar(
        "SELECT id FROM goals WHERE site_id = ? AND name_key = ? AND archived_at IS NULL AND id <> ?",
    )
    .bind(site.as_str())
    .bind(key)
    .bind(except.map_or("", GoalId::as_str))
    .fetch_optional(conn)
    .await?;
    Ok(taken.is_some())
}

impl GoalsHandlers for AppState {
    async fn goals_list(
        &self,
        ctx: &mut QueryCtx,
        args: GoalsListArgs,
    ) -> Result<Snapshot<SavedGoalList>, ApiError> {
        let scope = ctx.site_scope(&args.site, Capability::SiteRead).await?;
        let rows: Vec<GoalRow> = sqlx::query_as(
            "SELECT g.id, g.name, v.version, g.version, v.definition, g.archived_at IS NOT NULL, v.created_at
             FROM goals g JOIN goal_versions v ON v.goal_id = g.id AND v.version = g.version
             WHERE g.site_id = ? AND g.archived_at IS NULL ORDER BY g.name_key, g.id",
        )
        .bind(scope.site().as_str())
        .fetch_all(ctx.conn())
        .await?;
        let goals = rows
            .into_iter()
            .map(|row| saved(scope.site(), row))
            .collect::<Result<_, _>>()?;
        Ok(Snapshot {
            topic: Topic::site(scope.site()),
            data: SavedGoalList { goals },
        })
    }

    async fn goals_get(
        &self,
        ctx: &mut QueryCtx,
        args: GoalsGetArgs,
    ) -> Result<Snapshot<SavedGoalResult>, ApiError> {
        let scope = ctx.site_scope(&args.site, Capability::SiteRead).await?;
        let goal = goal(ctx.conn(), scope.site(), &args.goal, args.version).await?;
        Ok(Snapshot {
            topic: Topic::site(scope.site()),
            data: SavedGoalResult { goal },
        })
    }

    async fn goals_save(
        &self,
        ctx: &mut CommandCtx,
        args: GoalsSaveArgs,
    ) -> Result<SavedGoalResult, ApiError> {
        let scope = ctx
            .site_scope(&args.site, Capability::SiteDefinitionsWrite, None)
            .await?;
        let site = scope.site().clone();
        let user = ctx.person()?;
        let name = definition_name(&args.name, "name")?;
        let key = name.to_lowercase();
        let definition = GoalDefinition {
            goal_ref: None,
            ..args.definition
        };
        check(&definition, &site)?;
        if name_taken(ctx.conn(), &site, &key, args.goal.as_ref()).await? {
            return Err(FieldErrors::one("name", FieldErrorCode::InvalidValue, None));
        }
        let text = serde_json::to_string(&definition).map_err(ApiError::internal)?;
        let now = ctx.now();
        let (id, version) = match &args.goal {
            Some(id) => {
                let current = goal(ctx.conn(), &site, id, None).await?;
                if current.archived {
                    return Err(ApiError::not_found());
                }
                // Only a changed definition is a new version; a new name alone is not.
                let version = if current.definition == definition {
                    current.version
                } else {
                    current.version + 1
                };
                if version != current.version {
                    sqlx::query(
                        "INSERT INTO goal_versions (goal_id, version, definition, created_by, created_at)
                         VALUES (?, ?, ?, ?, ?)",
                    )
                    .bind(id.as_str())
                    .bind(i64::from(version))
                    .bind(&text)
                    .bind(user.as_str())
                    .bind(now)
                    .execute(ctx.conn())
                    .await?;
                }
                sqlx::query(
                    "UPDATE goals SET name = ?, name_key = ?, version = ?, updated_at = ? WHERE id = ?",
                )
                .bind(&name)
                .bind(&key)
                .bind(i64::from(version))
                .bind(now)
                .bind(id.as_str())
                .execute(ctx.conn())
                .await?;
                (id.clone(), version)
            }
            None => {
                let id = GoalId::new(crate::db::new_id());
                sqlx::query(
                    "INSERT INTO goals (id, site_id, name, name_key, version, created_by, created_at, updated_at)
                     VALUES (?, ?, ?, ?, 1, ?, ?, ?)",
                )
                .bind(id.as_str())
                .bind(site.as_str())
                .bind(&name)
                .bind(&key)
                .bind(user.as_str())
                .bind(now)
                .bind(now)
                .execute(ctx.conn())
                .await?;
                sqlx::query(
                    "INSERT INTO goal_versions (goal_id, version, definition, created_by, created_at)
                     VALUES (?, 1, ?, ?, ?)",
                )
                .bind(id.as_str())
                .bind(&text)
                .bind(user.as_str())
                .bind(now)
                .execute(ctx.conn())
                .await?;
                (id, 1)
            }
        };
        ctx.record(
            &Topic::site(&site),
            "goal.saved",
            json!({ "id": id, "version": version }),
        )
        .await?;
        let goal = goal(ctx.conn(), &site, &id, Some(version)).await?;
        Ok(SavedGoalResult { goal })
    }

    async fn goals_archive(
        &self,
        ctx: &mut CommandCtx,
        args: GoalsGetArgs,
    ) -> Result<SavedGoalResult, ApiError> {
        let scope = ctx
            .site_scope(&args.site, Capability::SiteDefinitionsWrite, None)
            .await?;
        let site = scope.site().clone();
        let archived = sqlx::query(
            "UPDATE goals SET archived_at = ?, updated_at = ? WHERE id = ? AND site_id = ? AND archived_at IS NULL",
        )
        .bind(ctx.now())
        .bind(ctx.now())
        .bind(args.goal.as_str())
        .bind(site.as_str())
        .execute(ctx.conn())
        .await?
        .rows_affected();
        if archived == 0 {
            return Err(ApiError::not_found());
        }
        ctx.record(
            &Topic::site(&site),
            "goal.archived",
            json!({ "id": args.goal }),
        )
        .await?;
        let goal = goal(ctx.conn(), &site, &args.goal, None).await?;
        Ok(SavedGoalResult { goal })
    }
}

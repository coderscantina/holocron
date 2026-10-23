//! Accounts, teams, sites and their configuration: the `control` step.
//!
//! The whole step commits in one control-database transaction with its inventory and checkpoint,
//! so an interrupted run either has all of it or none. Source IDs are kept wherever Holocron has
//! the same object, which makes every website, user, team, link, pixel, board, segment, report,
//! annotation and 3.x share keep its UUID. Objects without a source ID (personal workspaces,
//! 2.x website shares) get IDs derived from the source ID they belong to.

use std::collections::{BTreeMap, BTreeSet};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use holocron_analytics::identity::{SessionPolicy, SitePolicies};
use holocron_protocol::access::{InstallRole, Role, WorkspaceKind};
use holocron_protocol::ids::{BoardId, SiteId, UserId};
use holocron_protocol::limits::SHARE_LIFETIME_MAX_DAYS;
use holocron_protocol::operations::recordings::RecordingPolicy;
use holocron_protocol::operations::sharing::ShareTarget;
use serde_json::{Value, json};
use sqlx::SqliteConnection;

use super::definitions;
use super::signature::Layout;
use super::source::{Kind, RowExt, SourceRow};
use super::state::{self, Entry, Progress};
use super::{Ctx, derived_id};
use crate::identity;
use crate::init::Failure;
use crate::recordings::policy as recording;
use crate::sites::policy::insert_session_policy;

pub const STEP: &str = "control";
const DAY_MS: i64 = 86_400_000;
/// The inactivity window of imported sites until an operator changes it (0002 default).
const WINDOW_MINUTES: u16 = 30;

use Kind::{Flag, Int, Ms, Text};

/// Imported owners: who a source user or team became.
#[derive(Default)]
struct Owners {
    users: BTreeMap<String, (String, bool)>,
    teams: BTreeSet<String>,
    personal: BTreeMap<String, String>,
}

pub async fn import(ctx: &mut Ctx) -> Result<(), Failure> {
    if state::step(&ctx.db, STEP)
        .await
        .map_err(super::internal)?
        .done
    {
        return Ok(());
    }
    let mut tx = ctx
        .db
        .writer
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(super::internal)?;
    let mut inventory = Vec::new();
    let pg = ctx.layout == Layout::Umami3Postgresql;
    let mut owners = Owners::default();
    let mut rows = 0i64;

    // Accounts.
    let mut columns = vec![
        ("user_id", Text),
        ("username", Text),
        ("password", Text),
        ("role", Text),
        ("display_name", Text),
        ("logo_url", Text),
        ("created_at", Ms),
        ("deleted_at", Ms),
    ];
    if pg {
        columns.push(("two_factor_required", Flag));
    }
    let users = ctx
        .source
        .table("user", &columns, "", "user_id", &[])
        .await?;
    let mut emails: BTreeSet<String> = sqlx::query_scalar("SELECT email FROM users")
        .fetch_all(&mut *tx)
        .await
        .map_err(super::internal)?
        .into_iter()
        .collect();
    for user in &users {
        rows += 1;
        let id = user.text("user_id").unwrap_or_default().to_owned();
        let username = user.text("username").unwrap_or_default();
        if user.int("deleted_at").is_some() {
            inventory.push(Entry::new(
                "user",
                &id,
                "deleted_in_source",
                user.clone().into(),
            ));
            continue;
        }
        let email = login_email(username, &ctx.options.login_domain);
        if !emails.insert(email.clone()) {
            inventory.push(Entry::new(
                "user",
                &id,
                &format!("login_conflict:{email}"),
                user.clone().into(),
            ));
            continue;
        }
        let hash = match user.text("password").filter(|h| is_bcrypt(h)) {
            Some(hash) => hash.to_owned(),
            None => {
                inventory.push(Entry::new(
                    "user",
                    &id,
                    "password_not_convertible",
                    user.clone().into(),
                ));
                identity::hash_password(&identity::generate_password())
            }
        };
        let role = user.text("role").unwrap_or_default();
        let install_role = if role == "admin" {
            inventory.push(Entry::new(
                "user",
                &id,
                "install_admin_reads_no_analytics",
                user.clone().into(),
            ));
            InstallRole::InstallAdmin
        } else {
            InstallRole::User
        };
        if user.text("logo_url").is_some() {
            inventory.push(Entry::new(
                "user",
                &id,
                "logo_not_supported",
                user.clone().into(),
            ));
        }
        let name = user
            .text("display_name")
            .filter(|n| !n.trim().is_empty())
            .unwrap_or(username);
        sqlx::query(
            "INSERT INTO users (id, email, name, password_hash, install_role, created_at, two_factor_required)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&email)
        .bind(name)
        .bind(&hash)
        .bind(install_role.as_str())
        .bind(user.int("created_at").unwrap_or(ctx.clock_ms))
        .bind(user.flag("two_factor_required"))
        .execute(&mut *tx)
        .await
        .map_err(super::internal)?;
        owners
            .users
            .insert(id.clone(), (name.to_owned(), role == "view-only"));
    }

    // Teams and their members.
    let mut columns = vec![
        ("team_id", Text),
        ("name", Text),
        ("access_code", Text),
        ("logo_url", Text),
        ("created_at", Ms),
        ("deleted_at", Ms),
    ];
    if pg {
        columns.push(("two_factor_required", Flag));
    }
    for team in ctx
        .source
        .table("team", &columns, "", "team_id", &[])
        .await?
    {
        rows += 1;
        let id = team.text("team_id").unwrap_or_default().to_owned();
        if team.int("deleted_at").is_some() {
            inventory.push(Entry::new("team", &id, "deleted_in_source", team.into()));
            continue;
        }
        sqlx::query(
            "INSERT INTO workspaces (id, name, kind, created_at, two_factor_required) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(team.text("name").unwrap_or_default())
        .bind(WorkspaceKind::Team.as_str())
        .bind(team.int("created_at").unwrap_or(ctx.clock_ms))
        .bind(team.flag("two_factor_required"))
        .execute(&mut *tx)
        .await
        .map_err(super::internal)?;
        if team.text("access_code").is_some() {
            inventory.push(Entry::new(
                "team",
                &id,
                "access_code_not_supported",
                team.clone().into(),
            ));
        }
        if team.text("logo_url").is_some() {
            inventory.push(Entry::new(
                "team",
                &id,
                "logo_not_supported",
                team.clone().into(),
            ));
        }
        owners.teams.insert(id);
    }
    let members = ctx
        .source
        .table(
            "team_user",
            &[
                ("team_user_id", Text),
                ("team_id", Text),
                ("user_id", Text),
                ("role", Text),
            ],
            "",
            "team_user_id",
            &[],
        )
        .await?;
    for member in members {
        rows += 1;
        let key = member.text("team_user_id").unwrap_or_default().to_owned();
        let (team, user) = (
            member.text("team_id").unwrap_or_default(),
            member.text("user_id").unwrap_or_default(),
        );
        if !owners.teams.contains(team) || !owners.users.contains_key(user) {
            inventory.push(Entry::new(
                "team_user",
                key,
                "owner_not_imported",
                member.into(),
            ));
            continue;
        }
        let role = match member.text("role").unwrap_or_default() {
            "team-owner" => Role::Owner,
            "team-manager" => Role::Admin,
            "team-member" => {
                inventory.push(Entry::new(
                    "team_user",
                    &key,
                    "member_loses_site_delete",
                    member.clone().into(),
                ));
                Role::Member
            }
            "team-view-only" => Role::Viewer,
            other => {
                inventory.push(Entry::new(
                    "team_user",
                    key,
                    &format!("role_not_supported:{other}"),
                    member.into(),
                ));
                continue;
            }
        };
        sqlx::query("INSERT INTO memberships (workspace_id, user_id, role) VALUES (?, ?, ?)")
            .bind(team)
            .bind(user)
            .bind(role.as_str())
            .execute(&mut *tx)
            .await
            .map_err(super::internal)?;
    }

    // Websites.
    let mut columns = vec![
        ("website_id", Text),
        ("name", Text),
        ("domain", Text),
        ("user_id", Text),
        ("team_id", Text),
        ("created_at", Ms),
        ("deleted_at", Ms),
    ];
    if pg {
        columns.extend([("recorder_enabled", Flag), ("replay_config", Text)]);
    } else {
        columns.push(("share_id", Text));
    }
    let policy = SitePolicies::new(
        SessionPolicy::new(ctx.timezone, WINDOW_MINUTES).map_err(super::internal)?,
    );
    // Site ID to (workspace, author of definitions without one, site name).
    let mut sites: BTreeMap<String, (String, String, String)> = BTreeMap::new();
    let websites = ctx
        .source
        .table("website", &columns, "", "website_id", &[])
        .await?;
    for site in &websites {
        rows += 1;
        let id = site.text("website_id").unwrap_or_default().to_owned();
        if site.int("deleted_at").is_some() {
            inventory.push(Entry::new(
                "website",
                &id,
                "deleted_in_source",
                site.clone().into(),
            ));
            continue;
        }
        let Some((workspace, author)) = owner(&mut tx, &mut owners, site, ctx.clock_ms).await?
        else {
            inventory.push(Entry::new(
                "website",
                &id,
                "owner_not_imported",
                site.clone().into(),
            ));
            continue;
        };
        let name = site.text("name").unwrap_or_default().to_owned();
        sqlx::query(
            "INSERT INTO sites (id, workspace_id, name, domain, created_at, event_retention) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&workspace)
        .bind(&name)
        .bind(site.text("domain").unwrap_or_default())
        .bind(site.int("created_at").unwrap_or(ctx.clock_ms))
        .bind(ctx.options.retention.code())
        .execute(&mut *tx)
        .await
        .map_err(super::internal)?;
        insert_session_policy(&mut tx, &id, policy.current(), ctx.clock_ms)
            .await
            .map_err(super::internal)?;
        if site.flag("recorder_enabled") {
            let config: Value = site
                .text("replay_config")
                .and_then(|c| serde_json::from_str(c).ok())
                .unwrap_or(Value::Null);
            let (policy, lost) = recording_policy(&config);
            recording::store(&mut tx, &id, &policy, ctx.clock_ms)
                .await
                .map_err(super::internal)?;
            if !lost.is_empty() {
                inventory.push(Entry::partial("website", &id, site.clone().into(), &lost));
            }
        }
        if let Some(slug) = site.text("share_id") {
            // A 2.x public website share: `/share/<share_id>/<name>`.
            let share = derived_id("share", &id);
            let limits = definitions::share_limits(None);
            insert_share(
                &mut tx,
                ctx,
                ShareRow {
                    id: &share,
                    workspace: &workspace,
                    site: Some(&id),
                    board: None,
                    name: &format!("{name} (public link)"),
                    slug,
                    created_by: &author,
                    created_at: site.int("created_at").unwrap_or(ctx.clock_ms),
                },
                ShareTarget::Exploration {
                    state: Box::new(definitions::share_state(
                        &SiteId::new(id.clone()),
                        &ctx.options.timezone,
                    )),
                    exploration: None,
                    limits: limits.0,
                },
                &mut inventory,
                ("website", &id, site.clone().into(), &limits.1),
            )
            .await?;
        }
        sites.insert(id, (workspace, author, name));
    }

    if pg {
        rows += links_and_pixels(ctx, &mut tx, &mut owners, &mut inventory).await?;
        rows += annotations(ctx, &mut tx, &sites, &mut inventory).await?;
    }
    rows += segments(ctx, &mut tx, &sites, &mut inventory).await?;
    rows += reports(ctx, &mut tx, &sites, &owners, &mut inventory).await?;
    if pg {
        let boards = boards(ctx, &mut tx, &mut owners, &mut inventory, &mut rows).await?;
        rows += shares(ctx, &mut tx, &sites, &boards, &mut inventory).await?;
        rows += two_factor(ctx, &mut tx, &owners, &mut inventory).await?;
        rows += unmapped_tables(ctx, &mut inventory).await?;
        rows += settings(ctx, &mut tx, &mut inventory).await?;
    }

    for chunk in inventory.chunks(500) {
        state::record(&mut tx, chunk)
            .await
            .map_err(super::internal)?;
    }
    state::checkpoint(
        &mut tx,
        STEP,
        "",
        1,
        Progress {
            rows,
            imported: rows - inventory.iter().filter(|e| skips(&e.reason)).count() as i64,
            skipped: inventory.iter().filter(|e| skips(&e.reason)).count() as i64,
        },
        true,
    )
    .await
    .map_err(super::internal)?;
    tx.commit().await.map_err(super::internal)?;
    ctx.progress.checkpoint(STEP, rows, true);
    Ok(())
}

/// Inventory reasons whose object was not imported at all, as opposed to imported with a loss.
fn skips(reason: &str) -> bool {
    !matches!(
        reason,
        "partially_mapped"
            | "install_admin_reads_no_analytics"
            | "logo_not_supported"
            | "access_code_not_supported"
            | "member_loses_site_delete"
            | "password_not_convertible"
            | "two_factor_reenrollment_required"
            | "share_slug_guessable"
            | "visible_to_owner_only"
            | "view_only_personal_sites_read_only"
    )
}

/// `username` when it is an email address, else `username@<login_domain>`, lowercased.
pub fn login_email(username: &str, login_domain: &str) -> String {
    let login = identity::normalize_email(username);
    let login: String = login
        .chars()
        .map(|c| if c.is_whitespace() { '.' } else { c })
        .collect();
    if login.contains('@') {
        login
    } else {
        format!("{login}@{login_domain}")
    }
}

/// bcrypt as Umami writes it (`bcryptjs`, `$2a$`/`$2b$`, 60 characters).
fn is_bcrypt(hash: &str) -> bool {
    hash.len() == 60 && ["$2a$", "$2b$", "$2y$"].iter().any(|p| hash.starts_with(p))
}

/// The workspace of an object owned by a team or a user, and the user who authors definitions
/// without one. A user's personal workspace is created on first use with an ID derived from
/// the user ID; view-only users only read theirs.
async fn owner(
    conn: &mut SqliteConnection,
    owners: &mut Owners,
    row: &SourceRow,
    now: i64,
) -> Result<Option<(String, String)>, Failure> {
    let user = row
        .text("user_id")
        .or(row.text("created_by"))
        .unwrap_or_default();
    if let Some(team) = row.text("team_id") {
        return Ok(owners
            .teams
            .contains(team)
            .then(|| (team.to_owned(), user.to_owned())));
    }
    let Some((name, view_only)) = owners.users.get(user).cloned() else {
        return Ok(None);
    };
    if let Some(workspace) = owners.personal.get(user) {
        return Ok(Some((workspace.clone(), user.to_owned())));
    }
    let workspace = derived_id("personal", user);
    sqlx::query("INSERT INTO workspaces (id, name, kind, created_at) VALUES (?, ?, ?, ?)")
        .bind(&workspace)
        .bind(&name)
        .bind(WorkspaceKind::Personal.as_str())
        .bind(now)
        .execute(&mut *conn)
        .await
        .map_err(super::internal)?;
    let role = if view_only { Role::Viewer } else { Role::Owner };
    sqlx::query("INSERT INTO memberships (workspace_id, user_id, role) VALUES (?, ?, ?)")
        .bind(&workspace)
        .bind(user)
        .bind(role.as_str())
        .execute(&mut *conn)
        .await
        .map_err(super::internal)?;
    owners.personal.insert(user.to_owned(), workspace.clone());
    Ok(Some((workspace, user.to_owned())))
}

/// Umami's `replay_config` as a Holocron recording policy, and the keys it does not carry.
fn recording_policy(config: &Value) -> (RecordingPolicy, Vec<String>) {
    let mut policy = recording::disabled();
    policy.replay_enabled = true;
    policy.heatmap_enabled = true;
    policy.version = 1;
    if let Some(rate) = config["sampleRate"].as_f64() {
        // Umami stores a fraction of 1.
        let percent = if rate <= 1.0 { rate * 100.0 } else { rate };
        policy.replay_sample_percent = percent.round().clamp(1.0, 100.0) as u8;
    }
    let lost = config
        .as_object()
        .map(|c| {
            c.keys()
                .filter(|k| k.as_str() != "sampleRate")
                .map(|k| format!("replay_config.{k}"))
                .collect()
        })
        .unwrap_or_default();
    (policy, lost)
}

async fn links_and_pixels(
    ctx: &mut Ctx,
    conn: &mut SqliteConnection,
    owners: &mut Owners,
    inventory: &mut Vec<Entry>,
) -> Result<i64, Failure> {
    let mut rows = 0;
    for (table, with_url) in [("link", true), ("pixel", false)] {
        let mut columns = vec![
            (if with_url { "link_id" } else { "pixel_id" }, Text),
            ("name", Text),
            ("slug", Text),
            ("user_id", Text),
            ("team_id", Text),
            ("created_at", Ms),
            ("deleted_at", Ms),
        ];
        if with_url {
            columns.push(("url", Text));
        }
        let key = columns[0].0;
        for row in ctx.source.table(table, &columns, "", key, &[]).await? {
            rows += 1;
            let id = row.text(key).unwrap_or_default().to_owned();
            let Some((workspace, _)) = owner(conn, owners, &row, ctx.clock_ms).await? else {
                inventory.push(Entry::new(
                    if with_url { "link" } else { "pixel" },
                    id,
                    "owner_not_imported",
                    row.into(),
                ));
                continue;
            };
            let sql = if with_url {
                "INSERT INTO links (id, workspace_id, name, slug, created_at, deleted_at, event_retention, url) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
            } else {
                "INSERT INTO pixels (id, workspace_id, name, slug, created_at, deleted_at, event_retention) VALUES (?, ?, ?, ?, ?, ?, ?)"
            };
            let mut query = sqlx::query(sql)
                .bind(&id)
                .bind(&workspace)
                .bind(row.text("name").unwrap_or_default())
                .bind(row.text("slug").unwrap_or_default())
                .bind(row.int("created_at").unwrap_or(ctx.clock_ms))
                .bind(row.int("deleted_at"))
                .bind(ctx.options.retention.code());
            if with_url {
                query = query.bind(row.text("url").unwrap_or_default());
            }
            query.execute(&mut *conn).await.map_err(super::internal)?;
        }
    }
    Ok(rows)
}

async fn annotations(
    ctx: &mut Ctx,
    conn: &mut SqliteConnection,
    sites: &BTreeMap<String, (String, String, String)>,
    inventory: &mut Vec<Entry>,
) -> Result<i64, Failure> {
    let rows = ctx
        .source
        .table(
            "annotation",
            &[
                ("annotation_id", Text),
                ("website_id", Text),
                ("user_id", Text),
                ("date", Ms),
                ("all_day", Flag),
                ("note", Text),
                ("created_at", Ms),
            ],
            "",
            "annotation_id",
            &[],
        )
        .await?;
    let count = rows.len() as i64;
    for row in rows {
        let id = row.text("annotation_id").unwrap_or_default().to_owned();
        let Some((_, author, _)) = sites.get(row.text("website_id").unwrap_or_default()) else {
            inventory.push(Entry::new(
                "annotation",
                id,
                "site_not_imported",
                row.into(),
            ));
            continue;
        };
        if !row.flag("all_day") {
            inventory.push(Entry::new(
                "annotation",
                &id,
                "partially_mapped:all_day",
                row.clone().into(),
            ));
        }
        sqlx::query(
            "INSERT INTO annotations (id, site_id, at, text, created_by, created_at) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(row.text("website_id"))
        .bind(row.int("date").unwrap_or(ctx.clock_ms))
        .bind(row.text("note").unwrap_or_default())
        .bind(row.text("user_id").unwrap_or(author))
        .bind(row.int("created_at").unwrap_or(ctx.clock_ms))
        .execute(&mut *conn)
        .await
        .map_err(super::internal)?;
    }
    Ok(count)
}

async fn segments(
    ctx: &mut Ctx,
    conn: &mut SqliteConnection,
    sites: &BTreeMap<String, (String, String, String)>,
    inventory: &mut Vec<Entry>,
) -> Result<i64, Failure> {
    let rows = ctx
        .source
        .table(
            "segment",
            &[
                ("segment_id", Text),
                ("website_id", Text),
                ("type", Text),
                ("name", Text),
                ("parameters", Text),
                ("created_at", Ms),
            ],
            "",
            "segment_id",
            &[],
        )
        .await?;
    let count = rows.len() as i64;
    let mut names = BTreeSet::new();
    for row in rows {
        let id = row.text("segment_id").unwrap_or_default().to_owned();
        let site = row.text("website_id").unwrap_or_default().to_owned();
        let Some((_, author, _)) = sites.get(&site) else {
            inventory.push(Entry::new("segment", id, "site_not_imported", row.into()));
            continue;
        };
        if row.text("type") != Some("segment") {
            inventory.push(Entry::new(
                "segment",
                id,
                "cohort_not_supported",
                row.into(),
            ));
            continue;
        }
        let params = parse(row.text("parameters"));
        let (filter, lost) = match definitions::segment_filter(&params, &SiteId::new(site.clone()))
        {
            Ok(mapped) => mapped,
            Err(reason) => {
                inventory.push(Entry::new("segment", id, &reason, row.into()));
                continue;
            }
        };
        let name = row.text("name").unwrap_or_default().trim().to_owned();
        if !names.insert((site.clone(), name.to_lowercase())) {
            inventory.push(Entry::new("segment", id, "name_conflict", row.into()));
            continue;
        }
        if !lost.is_empty() {
            inventory.push(Entry::partial("segment", &id, row.clone().into(), &lost));
        }
        sqlx::query(
            "INSERT INTO segments (id, site_id, name, name_key, filter, created_by, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&site)
        .bind(&name)
        .bind(name.to_lowercase())
        .bind(serde_json::to_string(&filter).map_err(super::internal)?)
        .bind(author)
        .bind(row.int("created_at").unwrap_or(ctx.clock_ms))
        .execute(&mut *conn)
        .await
        .map_err(super::internal)?;
    }
    Ok(count)
}

async fn reports(
    ctx: &mut Ctx,
    conn: &mut SqliteConnection,
    sites: &BTreeMap<String, (String, String, String)>,
    owners: &Owners,
    inventory: &mut Vec<Entry>,
) -> Result<i64, Failure> {
    let rows = ctx
        .source
        .table(
            "report",
            &[
                ("report_id", Text),
                ("user_id", Text),
                ("website_id", Text),
                ("type", Text),
                ("name", Text),
                ("description", Text),
                ("parameters", Text),
                ("created_at", Ms),
                ("updated_at", Ms),
            ],
            "",
            "report_id",
            &[],
        )
        .await?;
    let count = rows.len() as i64;
    for row in rows {
        let id = row.text("report_id").unwrap_or_default().to_owned();
        let site = row.text("website_id").unwrap_or_default().to_owned();
        let owner = row.text("user_id").unwrap_or_default();
        if !sites.contains_key(&site) || !owners.users.contains_key(owner) {
            inventory.push(Entry::new("report", id, "owner_not_imported", row.into()));
            continue;
        }
        let params = parse(row.text("parameters"));
        let kind = row.text("type").unwrap_or_default();
        let (state, mut lost) = match definitions::report_state(
            &SiteId::new(site.clone()),
            &ctx.options.timezone,
            kind,
            &params,
        ) {
            Ok(mapped) => mapped,
            Err(reason) => {
                inventory.push(Entry::new("report", id, &reason, row.into()));
                continue;
            }
        };
        if row.text("description").is_some_and(|d| !d.is_empty()) {
            lost.push("description".into());
        }
        if !lost.is_empty() {
            inventory.push(Entry::partial("report", &id, row.clone().into(), &lost));
        }
        inventory.push(Entry::new(
            "report",
            &id,
            "visible_to_owner_only",
            row.clone().into(),
        ));
        let created = row.int("created_at").unwrap_or(ctx.clock_ms);
        sqlx::query(
            "INSERT INTO saved_explorations (id, site_id, owner_id, name, state, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&site)
        .bind(owner)
        .bind(row.text("name").unwrap_or_default())
        .bind(serde_json::to_string(&state).map_err(super::internal)?)
        .bind(created)
        .bind(row.int("updated_at").unwrap_or(created))
        .execute(&mut *conn)
        .await
        .map_err(super::internal)?;
    }
    Ok(count)
}

/// Boards with their workspace, by board ID.
async fn boards(
    ctx: &mut Ctx,
    conn: &mut SqliteConnection,
    owners: &mut Owners,
    inventory: &mut Vec<Entry>,
    rows: &mut i64,
) -> Result<BTreeMap<String, String>, Failure> {
    let source = ctx
        .source
        .table(
            "board",
            &[
                ("board_id", Text),
                ("type", Text),
                ("name", Text),
                ("description", Text),
                ("parameters", Text),
                ("user_id", Text),
                ("team_id", Text),
                ("created_at", Ms),
                ("updated_at", Ms),
            ],
            "",
            "board_id",
            &[],
        )
        .await?;
    let mut boards = BTreeMap::new();
    for row in source {
        *rows += 1;
        let id = row.text("board_id").unwrap_or_default().to_owned();
        let Some((workspace, author)) = owner(conn, owners, &row, ctx.clock_ms).await? else {
            inventory.push(Entry::new("board", id, "owner_not_imported", row.into()));
            continue;
        };
        let mut lost = Vec::new();
        let params = parse(row.text("parameters"));
        if params.as_object().is_some_and(|p| {
            p.values()
                .any(|v| v.as_array().is_some_and(|a| !a.is_empty()) || v.is_object())
        }) {
            lost.push("cards".to_owned());
        }
        if row.text("description").is_some_and(|d| !d.is_empty()) {
            lost.push("description".to_owned());
        }
        if !lost.is_empty() {
            inventory.push(Entry::partial("board", &id, row.clone().into(), &lost));
        }
        let created = row.int("created_at").unwrap_or(ctx.clock_ms);
        sqlx::query(
            "INSERT INTO boards (id, workspace_id, name, created_by, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&workspace)
        .bind(row.text("name").unwrap_or_default())
        .bind(&author)
        .bind(created)
        .bind(row.int("updated_at").unwrap_or(created))
        .execute(&mut *conn)
        .await
        .map_err(super::internal)?;
        boards.insert(id, workspace);
    }
    Ok(boards)
}

struct ShareRow<'a> {
    id: &'a str,
    workspace: &'a str,
    site: Option<&'a str>,
    board: Option<&'a str>,
    name: &'a str,
    slug: &'a str,
    created_by: &'a str,
    created_at: i64,
}

/// A public Umami share as a restricted share whose token is the old slug, so the old URL
/// `/share/<slug>` opens it. It expires after the longest lifetime Holocron allows.
async fn insert_share(
    conn: &mut SqliteConnection,
    ctx: &Ctx,
    share: ShareRow<'_>,
    target: ShareTarget,
    inventory: &mut Vec<Entry>,
    (table, key, row, lost): (&'static str, &str, Value, &[String]),
) -> Result<(), Failure> {
    if !lost.is_empty() {
        inventory.push(Entry::partial(table, key, row.clone(), lost));
    }
    // Umami generates 16 random characters; a shorter slug was chosen by hand.
    if share.slug.chars().count() < 16 {
        inventory.push(Entry::new(table, key, "share_slug_guessable", row));
    }
    let name: String = share.name.chars().take(100).collect();
    sqlx::query(
        "INSERT INTO shares (id, workspace_id, site_id, board_id, name, target, export, recordings,
                             prefix, token_hash, created_by, created_at, updated_at, expires_at)
         VALUES (?, ?, ?, ?, ?, ?, 0, 0, ?, ?, ?, ?, ?, ?)",
    )
    .bind(share.id)
    .bind(share.workspace)
    .bind(share.site)
    .bind(share.board)
    .bind(name)
    .bind(serde_json::to_string(&target).map_err(super::internal)?)
    .bind(share.slug.chars().take(8).collect::<String>())
    .bind(identity::token_digest(share.slug))
    .bind(share.created_by)
    .bind(share.created_at)
    .bind(ctx.clock_ms)
    .bind(ctx.clock_ms + i64::from(SHARE_LIFETIME_MAX_DAYS) * DAY_MS)
    .execute(conn)
    .await
    .map_err(super::internal)?;
    Ok(())
}

/// 3.x shares of websites and boards. Link and pixel shares have no Holocron counterpart.
async fn shares(
    ctx: &mut Ctx,
    conn: &mut SqliteConnection,
    sites: &BTreeMap<String, (String, String, String)>,
    boards: &BTreeMap<String, String>,
    inventory: &mut Vec<Entry>,
) -> Result<i64, Failure> {
    let rows = ctx
        .source
        .table(
            "share",
            &[
                ("share_id", Text),
                ("entity_id", Text),
                ("name", Text),
                ("share_type", Int),
                ("slug", Text),
                ("parameters", Text),
                ("created_at", Ms),
            ],
            "",
            "share_id",
            &[],
        )
        .await?;
    let count = rows.len() as i64;
    for row in rows {
        let id = row.text("share_id").unwrap_or_default().to_owned();
        let entity = row.text("entity_id").unwrap_or_default().to_owned();
        let params = parse(row.text("parameters"));
        let created_at = row.int("created_at").unwrap_or(ctx.clock_ms);
        let base = ShareRow {
            id: &id,
            workspace: "",
            site: None,
            board: None,
            name: row.text("name").unwrap_or_default(),
            slug: row.text("slug").unwrap_or_default(),
            created_by: "",
            created_at,
        };
        match row.int("share_type") {
            Some(1) => {
                let Some((workspace, author, _)) = sites.get(&entity) else {
                    inventory.push(Entry::new("share", id, "site_not_imported", row.into()));
                    continue;
                };
                let (limits, lost) = definitions::share_limits(Some(&params));
                let target = ShareTarget::Exploration {
                    state: Box::new(definitions::share_state(
                        &SiteId::new(entity.clone()),
                        &ctx.options.timezone,
                    )),
                    exploration: None,
                    limits,
                };
                let share = ShareRow {
                    workspace,
                    site: Some(&entity),
                    created_by: author,
                    ..base
                };
                insert_share(
                    conn,
                    ctx,
                    share,
                    target,
                    inventory,
                    ("share", &id, row.clone().into(), &lost),
                )
                .await?;
            }
            Some(4) => {
                let Some(workspace) = boards.get(&entity) else {
                    inventory.push(Entry::new("share", id, "board_not_imported", row.into()));
                    continue;
                };
                let readable: Vec<SiteId> = sites
                    .iter()
                    .filter(|(_, (w, _, _))| w == workspace)
                    .map(|(site, _)| SiteId::new(site.clone()))
                    .collect();
                let author: String =
                    sqlx::query_scalar("SELECT created_by FROM boards WHERE id = ?")
                        .bind(&entity)
                        .fetch_one(&mut *conn)
                        .await
                        .map_err(super::internal)?;
                let target = ShareTarget::Board {
                    board: BoardId::new(entity.clone()),
                    sites: readable,
                };
                let share = ShareRow {
                    workspace,
                    board: Some(&entity),
                    created_by: &author,
                    ..base
                };
                insert_share(
                    conn,
                    ctx,
                    share,
                    target,
                    inventory,
                    ("share", &id, row.clone().into(), &[]),
                )
                .await?;
            }
            _ => inventory.push(Entry::new(
                "share",
                id,
                "link_pixel_share_not_supported",
                row.into(),
            )),
        }
    }
    Ok(count)
}

/// Confirmed TOTP secrets, re-encrypted under `secrets.key` when the operator supplies Umami's
/// `TWO_FACTOR_ENCRYPTION_KEY`. Without it, or when a secret does not decrypt, the account must
/// enroll again at its first login. Recovery codes are bcrypt hashes Holocron cannot convert.
async fn two_factor(
    ctx: &mut Ctx,
    conn: &mut SqliteConnection,
    owners: &Owners,
    inventory: &mut Vec<Entry>,
) -> Result<i64, Failure> {
    let rows = ctx
        .source
        .table(
            "two_factor_auth",
            &[
                ("id", Text),
                ("user_id", Text),
                ("secret", Text),
                ("is_enabled", Flag),
                ("created_at", Ms),
            ],
            "",
            "id",
            &[],
        )
        .await?;
    let count = rows.len() as i64;
    for row in rows {
        let id = row.text("id").unwrap_or_default().to_owned();
        let user = row.text("user_id").unwrap_or_default().to_owned();
        if !owners.users.contains_key(&user) {
            inventory.push(Entry::new(
                "two_factor_auth",
                id,
                "owner_not_imported",
                row.into(),
            ));
            continue;
        }
        if !row.flag("is_enabled") {
            inventory.push(Entry::new(
                "two_factor_auth",
                id,
                "two_factor_not_enabled",
                row.into(),
            ));
            continue;
        }
        let secret = ctx
            .two_factor_key
            .and_then(|key| decrypt_totp(&key, row.text("secret").unwrap_or_default()));
        match secret {
            Some(secret) => {
                let sealed = ctx.secrets.seal(&UserId::new(user.clone()), &secret);
                sqlx::query(
                    "INSERT INTO two_factor (user_id, secret, confirmed_at) VALUES (?, ?, ?)",
                )
                .bind(&user)
                .bind(sealed)
                .bind(row.int("created_at").unwrap_or(ctx.clock_ms))
                .execute(&mut *conn)
                .await
                .map_err(super::internal)?;
            }
            None => {
                sqlx::query("UPDATE users SET two_factor_required = 1 WHERE id = ?")
                    .bind(&user)
                    .execute(&mut *conn)
                    .await
                    .map_err(super::internal)?;
                inventory.push(Entry::new(
                    "two_factor_auth",
                    id,
                    "two_factor_reenrollment_required",
                    row.into(),
                ));
            }
        }
    }
    Ok(count)
}

/// AES-256-GCM `ciphertext:iv:tag` hex, as Umami's `encryptSecret` writes it, holding a base32
/// TOTP secret. Returns the secret's bytes.
pub fn decrypt_totp(key: &[u8; 32], stored: &str) -> Option<Vec<u8>> {
    let hex = |s: &str| data_encoding::HEXLOWER_PERMISSIVE.decode(s.as_bytes()).ok();
    let mut parts = stored.split(':');
    let (ciphertext, iv, tag) = (
        hex(parts.next()?)?,
        hex(parts.next()?)?,
        hex(parts.next()?)?,
    );
    let iv: [u8; 12] = iv.try_into().ok()?;
    if parts.next().is_some() || tag.len() != 16 {
        return None;
    }
    let cipher = Aes256Gcm::new_from_slice(key).ok()?;
    let plain = cipher
        .decrypt(&Nonce::from(iv), [ciphertext, tag].concat().as_slice())
        .ok()?;
    let base32 = String::from_utf8(plain).ok()?.trim().to_uppercase();
    data_encoding::BASE32_NOPAD
        .decode(base32.trim_end_matches('=').as_bytes())
        .ok()
        .filter(|secret| !secret.is_empty())
}

/// Tables nothing in Holocron corresponds to, kept row by row: API keys (reissued at cutover),
/// recovery codes, and transient 2FA state.
async fn unmapped_tables(ctx: &mut Ctx, inventory: &mut Vec<Entry>) -> Result<i64, Failure> {
    type Columns = &'static [(&'static str, Kind)];
    let tables: [(&'static str, &str, Columns, &str); 4] = [
        (
            "api_key",
            "api_key_id",
            &[
                ("api_key_id", Text),
                ("user_id", Text),
                ("name", Text),
                ("key_hash", Text),
                ("key_prefix", Text),
                ("last_used_at", Ms),
                ("created_at", Ms),
            ],
            "api_key_reissue_required",
        ),
        (
            "two_factor_backup_code",
            "id",
            &[
                ("id", Text),
                ("user_id", Text),
                ("code_hash", Text),
                ("used", Flag),
                ("created_at", Ms),
            ],
            "recovery_codes_not_convertible",
        ),
        (
            "two_factor_otp_used",
            "id",
            &[
                ("id", Text),
                ("user_id", Text),
                ("otp", Text),
                ("expires_at", Ms),
            ],
            "transient_not_imported",
        ),
        (
            "two_factor_rate_limit",
            "id",
            &[
                ("id", Text),
                ("user_id", Text),
                ("attempts", Int),
                ("locked_until", Ms),
                ("updated_at", Ms),
            ],
            "transient_not_imported",
        ),
    ];
    let mut count = 0;
    for (table, key, columns, reason) in tables {
        for row in ctx.source.table(table, columns, "", key, &[]).await? {
            count += 1;
            let id = row.text(key).unwrap_or_default().to_owned();
            inventory.push(Entry::new(table, id, reason, row.into()));
        }
    }
    Ok(count)
}

/// `twoFactorRequiredGlobal` becomes the installation's 2FA requirement; other settings are kept
/// in the inventory.
async fn settings(
    ctx: &mut Ctx,
    conn: &mut SqliteConnection,
    inventory: &mut Vec<Entry>,
) -> Result<i64, Failure> {
    let rows = ctx
        .source
        .table(
            "app_setting",
            &[("key", Text), ("value", Text)],
            "",
            "\"key\"",
            &[],
        )
        .await?;
    let count = rows.len() as i64;
    for row in rows {
        let key = row.text("key").unwrap_or_default().to_owned();
        if key == "twoFactorRequiredGlobal" {
            sqlx::query("UPDATE installation_settings SET two_factor_required = ?, updated_at = ? WHERE id = 1")
                .bind(row.text("value") == Some("true"))
                .bind(ctx.clock_ms)
                .execute(&mut *conn)
                .await
                .map_err(super::internal)?;
        } else {
            inventory.push(Entry::new(
                "app_setting",
                key,
                "setting_not_supported",
                row.into(),
            ));
        }
    }
    Ok(count)
}

fn parse(text: Option<&str>) -> Value {
    text.and_then(|t| serde_json::from_str(t).ok())
        .unwrap_or_else(|| json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usernames_become_logins() {
        assert_eq!(login_email("Admin", "umami.invalid"), "admin@umami.invalid");
        assert_eq!(login_email("Jo Doe", "corp.example"), "jo.doe@corp.example");
        assert_eq!(login_email(" Mike@Example.org ", "x"), "mike@example.org");
    }

    #[test]
    fn umami_totp_secrets_decrypt_with_the_operator_key() {
        // Encrypted like Umami's encryptSecret: AES-256-GCM, `ciphertext:iv:tag` in hex.
        let key = [7u8; 32];
        let iv = [9u8; 12];
        let base32 = "JBSWY3DPEHPK3PXP";
        let sealed = Aes256Gcm::new_from_slice(&key)
            .unwrap()
            .encrypt(&Nonce::from(iv), base32.as_bytes())
            .unwrap();
        let (ciphertext, tag) = sealed.split_at(sealed.len() - 16);
        let hex = data_encoding::HEXLOWER;
        let stored = format!(
            "{}:{}:{}",
            hex.encode(ciphertext),
            hex.encode(&iv),
            hex.encode(tag)
        );
        assert_eq!(
            decrypt_totp(&key, &stored).unwrap(),
            b"Hello!\xde\xad\xbe\xef"
        );
        assert_eq!(decrypt_totp(&[8u8; 32], &stored), None);
        assert_eq!(decrypt_totp(&key, "synthetic-encrypted-secret"), None);
    }
}

//! `links.*` and `pixels.*`: the links and pixels of a workspace (decision 0020). A role has the
//! capabilities on them that it has on a site of the workspace. Both kinds share one table
//! layout and differ only in the link's URL, so every handler is one of the functions below
//! called with its [`Kind`].

use holocron_protocol::access::{Capability, WorkspaceCapability};
use holocron_protocol::envelope::Topic;
use holocron_protocol::errors::FieldErrorCode;
use holocron_protocol::exploration::{
    ExplorationState, ExploreResult, Measure, NamedMeasure, Panel,
};
use holocron_protocol::ids::{LinkId, PixelId, WorkspaceId};
use holocron_protocol::limits::{LINK_URL_MAX_CHARS, SLUG_GENERATED_CHARS, SLUG_MAX_CHARS};
use holocron_protocol::operations::explorations::{Deleted, ExploreArgs};
use holocron_protocol::operations::links::{
    Link, LinkArgs, LinkList, LinkResult, LinksCreateArgs, LinksHandlers, LinksUpdateArgs, Pixel,
    PixelArgs, PixelList, PixelResult, PixelsCreateArgs, PixelsUpdateArgs, SourcesListArgs,
};
use holocron_protocol::registry::Snapshot;
use serde_json::json;
use sqlx::SqliteConnection;
use url::Url;
use uuid::Uuid;

use crate::context::{CommandCtx, QueryCtx};
use crate::error::{ApiError, FieldErrors};
use crate::explorations::{definition_name, parse, refused};
use crate::maintenance::jobs::{self, Job};
use crate::retention::{self, Kind};
use crate::state::AppState;

/// Characters a generated slug is made of: lowercase letters and digits, easy to read aloud.
const SLUG_ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";

/// One stored link or pixel; `url` only for links.
#[derive(sqlx::FromRow)]
struct Row {
    id: String,
    workspace_id: String,
    name: String,
    slug: String,
    url: Option<String>,
    lost_hits: i64,
    created_at: i64,
}

impl Row {
    fn link(self) -> Link {
        Link {
            id: LinkId::new(self.id),
            workspace: WorkspaceId::new(self.workspace_id),
            name: self.name,
            slug: self.slug,
            url: self.url.unwrap_or_default(),
            lost_hits: self.lost_hits,
            created_at: self.created_at,
        }
    }

    fn pixel(self) -> Pixel {
        Pixel {
            id: PixelId::new(self.id),
            workspace: WorkspaceId::new(self.workspace_id),
            name: self.name,
            slug: self.slug,
            lost_hits: self.lost_hits,
            created_at: self.created_at,
        }
    }
}

fn noun(kind: Kind) -> &'static str {
    match kind {
        Kind::Link => "link",
        _ => "pixel",
    }
}

/// The columns of [`Row`]; pixels have no URL.
fn columns(kind: Kind) -> &'static str {
    match kind {
        Kind::Link => "id, workspace_id, name, slug, url, lost_hits, created_at",
        _ => "id, workspace_id, name, slug, NULL AS url, lost_hits, created_at",
    }
}

/// A live link or pixel; a deleted or missing one is not found.
async fn load(conn: &mut SqliteConnection, kind: Kind, id: &str) -> Result<Row, ApiError> {
    let found: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {} FROM {} WHERE id = ? AND deleted_at IS NULL",
        columns(kind),
        kind.table()
    )))
    .bind(id)
    .fetch_optional(conn)
    .await?;
    found.ok_or_else(ApiError::not_found)
}

async fn list(
    conn: &mut SqliteConnection,
    kind: Kind,
    workspace: &WorkspaceId,
) -> Result<Vec<Row>, ApiError> {
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {} FROM {} WHERE workspace_id = ? AND deleted_at IS NULL ORDER BY lower(name), id",
        columns(kind),
        kind.table()
    )))
    .bind(workspace.as_str())
    .fetch_all(conn)
    .await?)
}

/// A slug as given, after trimming: 1 to [`SLUG_MAX_CHARS`] ASCII letters, digits, `-` or `_`,
/// so it stays one URL path segment that needs no encoding.
fn checked_slug(slug: &str) -> Result<String, ApiError> {
    let slug = slug.trim();
    if slug.is_empty() {
        return Err(FieldErrors::one("slug", FieldErrorCode::Required, None));
    }
    if slug.len() > SLUG_MAX_CHARS {
        return Err(FieldErrors::one(
            "slug",
            FieldErrorCode::TooLong,
            Some(SLUG_MAX_CHARS as u32),
        ));
    }
    if !slug
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(FieldErrors::one(
            "slug",
            FieldErrorCode::InvalidFormat,
            None,
        ));
    }
    Ok(slug.to_owned())
}

fn generated_slug() -> Result<String, ApiError> {
    let mut bytes = [0u8; SLUG_GENERATED_CHARS];
    getrandom::fill(&mut bytes).map_err(ApiError::internal)?;
    // 256 is a multiple of neither 36 nor a power of it; the slight bias does not matter for
    // a slug, which is public and only has to be unlikely to collide.
    Ok(bytes
        .iter()
        .map(|b| SLUG_ALPHABET[usize::from(*b) % SLUG_ALPHABET.len()] as char)
        .collect())
}

/// Whether another link (or pixel) holds `slug`. Deleted imported ones keep theirs.
async fn slug_taken(
    conn: &mut SqliteConnection,
    kind: Kind,
    slug: &str,
    except: Option<&str>,
) -> Result<bool, ApiError> {
    let taken: Option<i64> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT 1 FROM {} WHERE slug = ? AND id IS NOT ?",
        kind.table()
    )))
    .bind(slug)
    .bind(except)
    .fetch_optional(conn)
    .await?;
    Ok(taken.is_some())
}

/// The slug a create or update stores: the given one when it is valid and free, else a fresh
/// one when none was given.
async fn free_slug(
    conn: &mut SqliteConnection,
    kind: Kind,
    given: Option<&str>,
    except: Option<&str>,
) -> Result<String, ApiError> {
    if let Some(given) = given {
        let slug = checked_slug(given)?;
        if slug_taken(conn, kind, &slug, except).await? {
            return Err(FieldErrors::one("slug", FieldErrorCode::InvalidValue, None));
        }
        return Ok(slug);
    }
    // 36^8 slugs: a collision is rare, a second one in a row rarer still.
    for _ in 0..3 {
        let slug = generated_slug()?;
        if !slug_taken(conn, kind, &slug, None).await? {
            return Ok(slug);
        }
    }
    Err(ApiError::internal("no free slug after three tries"))
}

/// A link target: an absolute `http` or `https` URL with a host and without credentials, in
/// its normalized form. Credentials are refused because `https://bank.example@evil.example`
/// reads like the bank. The target comes only from people who may change the link, never from
/// the request that follows it, so `/q/<slug>` is not an open redirect.
fn checked_url(url: &str) -> Result<String, ApiError> {
    let url = url.trim();
    if url.is_empty() {
        return Err(FieldErrors::one("url", FieldErrorCode::Required, None));
    }
    if url.chars().count() > LINK_URL_MAX_CHARS {
        return Err(FieldErrors::one(
            "url",
            FieldErrorCode::TooLong,
            Some(LINK_URL_MAX_CHARS as u32),
        ));
    }
    let parsed = Url::parse(url)
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https"))
        .filter(|u| u.host_str().is_some_and(|host| !host.is_empty()))
        .filter(|u| u.username().is_empty() && u.password().is_none())
        .ok_or_else(|| FieldErrors::one("url", FieldErrorCode::InvalidFormat, None))?;
    // The normalized form is ASCII, so it is always a valid `Location` header.
    let normalized = String::from(parsed);
    if normalized.len() > LINK_URL_MAX_CHARS {
        return Err(FieldErrors::one(
            "url",
            FieldErrorCode::TooLong,
            Some(LINK_URL_MAX_CHARS as u32),
        ));
    }
    Ok(normalized)
}

/// Link and pixel reports count hits as events, and visitors by their daily pseudonyms (UTC
/// days). Measures that need pageviews or sessions, and reports, are refused (0004 Sources).
fn checked_hit_state(state: &ExplorationState, panels: &[Panel]) -> Result<(), ApiError> {
    let hit_measure = |m: &Measure| {
        matches!(
            m,
            Measure::Named(
                NamedMeasure::Events | NamedMeasure::Visitors | NamedMeasure::VisitorDays
            )
        )
    };
    let mut measures = state
        .measures
        .iter()
        .chain(panels.iter().flat_map(|p| &p.measures));
    if state.report.is_some() || !measures.all(hit_measure) {
        return Err(FieldErrors::one(
            "state",
            FieldErrorCode::InvalidValue,
            None,
        ));
    }
    Ok(())
}

impl AppState {
    async fn sources_list(
        &self,
        ctx: &mut QueryCtx,
        kind: Kind,
        workspace: &WorkspaceId,
    ) -> Result<(Vec<Row>, Vec<Capability>), ApiError> {
        let scope = ctx.sources_scope(workspace, Capability::SiteRead).await?;
        let rows = list(ctx.conn(), kind, workspace).await?;
        Ok((rows, scope.source_capabilities().to_vec()))
    }

    async fn source_create(
        &self,
        ctx: &mut CommandCtx,
        kind: Kind,
        workspace: &WorkspaceId,
        name: &str,
        slug: Option<&str>,
        url: Option<&str>,
    ) -> Result<Row, ApiError> {
        ctx.workspace_scope(workspace, WorkspaceCapability::Write, None)
            .await?;
        let name = definition_name(name, "name")?;
        let url = url.map(checked_url).transpose()?;
        let slug = free_slug(ctx.conn(), kind, slug, None).await?;
        let id = crate::db::new_id();
        let sql = format!(
            "INSERT INTO {} (id, workspace_id, name, slug, created_at{}) VALUES (?, ?, ?, ?, ?{})",
            kind.table(),
            if url.is_some() { ", url" } else { "" },
            if url.is_some() { ", ?" } else { "" },
        );
        let mut insert = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(&id)
            .bind(workspace.as_str())
            .bind(&name)
            .bind(&slug)
            .bind(ctx.now());
        if let Some(url) = &url {
            insert = insert.bind(url);
        }
        insert.execute(ctx.conn()).await?;
        ctx.record(
            &Topic::workspace(workspace),
            &format!("{}.created", noun(kind)),
            json!({ "id": id }),
        )
        .await?;
        load(ctx.conn(), kind, &id).await
    }

    async fn source_update(
        &self,
        ctx: &mut CommandCtx,
        kind: Kind,
        id: &str,
        name: &str,
        slug: &str,
        url: Option<&str>,
    ) -> Result<Row, ApiError> {
        let current = load(ctx.conn(), kind, id).await?;
        let workspace = WorkspaceId::new(current.workspace_id);
        ctx.sources_scope(&workspace, Capability::SiteConfigure)
            .await?;
        let name = definition_name(name, "name")?;
        let url = url.map(checked_url).transpose()?;
        let slug = free_slug(ctx.conn(), kind, Some(slug), Some(id)).await?;
        let sql = format!(
            "UPDATE {} SET name = ?, slug = ?{} WHERE id = ?",
            kind.table(),
            if url.is_some() { ", url = ?" } else { "" },
        );
        let mut update = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(&name)
            .bind(&slug);
        if let Some(url) = &url {
            update = update.bind(url);
        }
        update.bind(id).execute(ctx.conn()).await?;
        ctx.record(
            &Topic::workspace(&workspace),
            &format!("{}.updated", noun(kind)),
            json!({ "id": id }),
        )
        .await?;
        load(ctx.conn(), kind, id).await
    }

    /// Deletes the row, which frees its slug, and erases the hits with the job that erases a
    /// deleted site: it works on any analytical source.
    async fn source_delete(
        &self,
        ctx: &mut CommandCtx,
        kind: Kind,
        id: &str,
    ) -> Result<Deleted, ApiError> {
        let current = load(ctx.conn(), kind, id).await?;
        let workspace = WorkspaceId::new(current.workspace_id);
        ctx.sources_scope(&workspace, Capability::SiteDelete)
            .await?;
        ctx.require_fresh_login()?;
        let source = Uuid::parse_str(id).map_err(ApiError::internal)?;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {} WHERE id = ?",
            kind.table()
        )))
        .bind(id)
        .execute(ctx.conn())
        .await?;
        // Committed with the deletion, so the erasure runs even after a crash right after it.
        let now = ctx.now();
        jobs::enqueue(ctx.conn(), &Job::EraseSite { site: source }, now).await?;
        self.maintenance.wake.notify_one();
        ctx.record(
            &Topic::workspace(&workspace),
            &format!("{}.deleted", noun(kind)),
            json!({ "id": id }),
        )
        .await?;
        Ok(Deleted { deleted: true })
    }

    async fn source_explore(
        &self,
        ctx: &mut QueryCtx,
        kind: Kind,
        args: ExploreArgs,
    ) -> Result<Snapshot<ExploreResult>, ApiError> {
        let state: ExplorationState = parse(&args.state, "state")?;
        let panels = args
            .panels
            .iter()
            .map(|panel| parse::<Panel>(panel, "panels"))
            .collect::<Result<Vec<_>, _>>()?;
        let source = load(ctx.conn(), kind, state.site.as_str()).await?;
        let workspace = WorkspaceId::new(source.workspace_id);
        ctx.sources_scope(&workspace, Capability::SiteRead).await?;
        checked_hit_state(&state, &panels)?;
        let query = holocron_analytics::explore::validate(&state, &panels).map_err(refused)?;
        let retention = retention::effective(ctx.conn(), kind, &source.id)
            .await?
            .ok_or_else(ApiError::not_found)?;
        let result = self
            .explore_source(&source.id, Vec::new(), retention, query, false)
            .await?;
        Ok(Snapshot {
            topic: Topic::workspace(&workspace),
            data: result,
        })
    }
}

impl LinksHandlers for AppState {
    async fn links_list(
        &self,
        ctx: &mut QueryCtx,
        args: SourcesListArgs,
    ) -> Result<Snapshot<LinkList>, ApiError> {
        let (rows, capabilities) = self.sources_list(ctx, Kind::Link, &args.workspace).await?;
        Ok(Snapshot {
            topic: Topic::workspace(&args.workspace),
            data: LinkList {
                links: rows.into_iter().map(Row::link).collect(),
                capabilities,
            },
        })
    }

    async fn links_explore(
        &self,
        ctx: &mut QueryCtx,
        args: ExploreArgs,
    ) -> Result<Snapshot<ExploreResult>, ApiError> {
        self.source_explore(ctx, Kind::Link, args).await
    }

    async fn links_create(
        &self,
        ctx: &mut CommandCtx,
        args: LinksCreateArgs,
    ) -> Result<LinkResult, ApiError> {
        let row = self
            .source_create(
                ctx,
                Kind::Link,
                &args.workspace,
                &args.name,
                args.slug.as_deref(),
                Some(&args.url),
            )
            .await?;
        Ok(LinkResult { link: row.link() })
    }

    async fn links_update(
        &self,
        ctx: &mut CommandCtx,
        args: LinksUpdateArgs,
    ) -> Result<LinkResult, ApiError> {
        let row = self
            .source_update(
                ctx,
                Kind::Link,
                args.link.as_str(),
                &args.name,
                &args.slug,
                Some(&args.url),
            )
            .await?;
        Ok(LinkResult { link: row.link() })
    }

    async fn links_delete(
        &self,
        ctx: &mut CommandCtx,
        args: LinkArgs,
    ) -> Result<Deleted, ApiError> {
        self.source_delete(ctx, Kind::Link, args.link.as_str())
            .await
    }

    async fn pixels_list(
        &self,
        ctx: &mut QueryCtx,
        args: SourcesListArgs,
    ) -> Result<Snapshot<PixelList>, ApiError> {
        let (rows, capabilities) = self.sources_list(ctx, Kind::Pixel, &args.workspace).await?;
        Ok(Snapshot {
            topic: Topic::workspace(&args.workspace),
            data: PixelList {
                pixels: rows.into_iter().map(Row::pixel).collect(),
                capabilities,
            },
        })
    }

    async fn pixels_explore(
        &self,
        ctx: &mut QueryCtx,
        args: ExploreArgs,
    ) -> Result<Snapshot<ExploreResult>, ApiError> {
        self.source_explore(ctx, Kind::Pixel, args).await
    }

    async fn pixels_create(
        &self,
        ctx: &mut CommandCtx,
        args: PixelsCreateArgs,
    ) -> Result<PixelResult, ApiError> {
        let row = self
            .source_create(
                ctx,
                Kind::Pixel,
                &args.workspace,
                &args.name,
                args.slug.as_deref(),
                None,
            )
            .await?;
        Ok(PixelResult { pixel: row.pixel() })
    }

    async fn pixels_update(
        &self,
        ctx: &mut CommandCtx,
        args: PixelsUpdateArgs,
    ) -> Result<PixelResult, ApiError> {
        let row = self
            .source_update(
                ctx,
                Kind::Pixel,
                args.pixel.as_str(),
                &args.name,
                &args.slug,
                None,
            )
            .await?;
        Ok(PixelResult { pixel: row.pixel() })
    }

    async fn pixels_delete(
        &self,
        ctx: &mut CommandCtx,
        args: PixelArgs,
    ) -> Result<Deleted, ApiError> {
        self.source_delete(ctx, Kind::Pixel, args.pixel.as_str())
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_one_plain_path_segment() {
        assert_eq!(checked_slug(" spring-sale_2 ").unwrap(), "spring-sale_2");
        for bad in [
            "",
            "a/b",
            "a b",
            "ümlaut",
            "a?b",
            &"x".repeat(SLUG_MAX_CHARS + 1),
        ] {
            assert!(checked_slug(bad).is_err(), "{bad}");
        }
        let generated = generated_slug().unwrap();
        assert_eq!(generated.len(), SLUG_GENERATED_CHARS);
        assert!(checked_slug(&generated).is_ok());
    }

    #[test]
    fn targets_are_http_urls_without_credentials() {
        assert_eq!(
            checked_url("https://example.com").unwrap(),
            "https://example.com/"
        );
        assert_eq!(
            checked_url("https://bücher.example/ä?x=1").unwrap(),
            "https://xn--bcher-kva.example/%C3%A4?x=1"
        );
        for bad in [
            "",
            "example.com",
            "/relative",
            "javascript:alert(1)",
            "data:text/html,hi",
            "ftp://example.com/",
            "https://bank.example@evil.example/",
            "https://user:pass@example.com/",
        ] {
            assert!(checked_url(bad).is_err(), "{bad}");
        }
    }
}

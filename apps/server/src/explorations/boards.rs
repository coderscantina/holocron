//! `boards.*`: workspace boards of exploration cards. Any member reads them (`workspace.read`);
//! roles above viewer change them (`workspace.write`). Keys reach boards only with every site
//! (decision 0012). A board share reads its own board, with cards of other sites unavailable.
//! A card's state must be a site of the board's workspace the caller can read. Board commands
//! record on the `workspace:<id>` topic, which is also the expected revision.

use holocron_protocol::access::{Capability, WorkspaceCapability};
use holocron_protocol::envelope::Topic;
use holocron_protocol::errors::FieldErrorCode;
use holocron_protocol::ids::{BoardId, CardId, ExplorationId, SiteId, WorkspaceId};
use holocron_protocol::limits::BOARD_CARDS_MAX;
use holocron_protocol::operations::boards::{
    Board, BoardArgs, BoardList, BoardResult, BoardSummary, BoardsCreateArgs, BoardsHandlers,
    BoardsListArgs, BoardsPinArgs, BoardsUpdateArgs, Card,
};
use holocron_protocol::operations::explorations::Deleted;
use holocron_protocol::operations::sharing::ShareTarget;
use holocron_protocol::registry::Snapshot;
use serde_json::json;
use sqlx::SqliteConnection;

use super::{checked_state, definition_name};
use crate::context::{CommandCtx, QueryCtx};
use crate::error::{ApiError, FieldErrors};
use crate::state::AppState;

/// The workspace a board belongs to; a missing board is not found.
async fn board_workspace(
    conn: &mut SqliteConnection,
    board: &BoardId,
) -> Result<WorkspaceId, ApiError> {
    let workspace: Option<String> =
        sqlx::query_scalar("SELECT workspace_id FROM boards WHERE id = ?")
            .bind(board.as_str())
            .fetch_optional(conn)
            .await?;
    workspace
        .map(WorkspaceId::new)
        .ok_or_else(ApiError::not_found)
}

/// A board with its cards. With `reach`, cards of other sites are unavailable: no state and no
/// exploration, the same as a card whose site the caller cannot read.
async fn load(
    conn: &mut SqliteConnection,
    board: &BoardId,
    can_edit: bool,
    reach: Option<&[SiteId]>,
) -> Result<Board, ApiError> {
    let (workspace, name, updated_at): (String, String, i64) =
        sqlx::query_as("SELECT workspace_id, name, updated_at FROM boards WHERE id = ?")
            .bind(board.as_str())
            .fetch_one(&mut *conn)
            .await?;
    let rows: Vec<(String, String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT id, title, site_id, state, exploration_id FROM board_cards WHERE board_id = ? ORDER BY position",
    )
    .bind(board.as_str())
    .fetch_all(conn)
    .await?;
    let cards = rows
        .into_iter()
        .map(|(id, title, site, state, exploration)| {
            let available = reach.is_none_or(|sites| sites.iter().any(|s| s.as_str() == site));
            Ok(Card {
                id: CardId::new(id),
                title,
                state: if available {
                    Some(serde_json::from_str(&state).map_err(ApiError::internal)?)
                } else {
                    None
                },
                exploration: exploration.filter(|_| available).map(ExplorationId::new),
            })
        })
        .collect::<Result<_, ApiError>>()?;
    Ok(Board {
        id: board.clone(),
        workspace: WorkspaceId::new(workspace),
        name,
        cards,
        can_edit,
        updated_at,
    })
}

impl BoardsHandlers for AppState {
    async fn boards_list(
        &self,
        ctx: &mut QueryCtx,
        args: BoardsListArgs,
    ) -> Result<Snapshot<BoardList>, ApiError> {
        let scope = ctx
            .workspace_scope(&args.workspace, WorkspaceCapability::Read)
            .await?;
        let rows: Vec<(String, String, i64, i64)> = sqlx::query_as(
            "SELECT b.id, b.name, (SELECT count(*) FROM board_cards c WHERE c.board_id = b.id), b.updated_at
             FROM boards b WHERE b.workspace_id = ? ORDER BY lower(b.name), b.id",
        )
        .bind(args.workspace.as_str())
        .fetch_all(ctx.conn())
        .await?;
        Ok(Snapshot {
            topic: Topic::workspace(&args.workspace),
            data: BoardList {
                boards: rows
                    .into_iter()
                    .map(|(id, name, cards, updated_at)| BoardSummary {
                        id: BoardId::new(id),
                        name,
                        cards,
                        updated_at,
                    })
                    .collect(),
                can_edit: scope.can(WorkspaceCapability::Write),
            },
        })
    }

    async fn boards_get(
        &self,
        ctx: &mut QueryCtx,
        args: BoardArgs,
    ) -> Result<Snapshot<BoardResult>, ApiError> {
        let workspace = board_workspace(ctx.conn(), &args.board).await?;
        let shared = ctx
            .caller()
            .share()
            .map(|share| (share.workspace.clone(), share.target.clone()));
        let board = match shared {
            // A board share reads its own board, in its own workspace, on its listed sites.
            Some((home, ShareTarget::Board { board, sites }))
                if board == args.board && home == workspace =>
            {
                load(ctx.conn(), &args.board, false, Some(&sites)).await?
            }
            Some(_) => return Err(ApiError::not_found()),
            None => {
                let scope = ctx
                    .workspace_scope(&workspace, WorkspaceCapability::Read)
                    .await?;
                let can_edit = scope.can(WorkspaceCapability::Write);
                load(ctx.conn(), &args.board, can_edit, None).await?
            }
        };
        Ok(Snapshot {
            topic: Topic::workspace(&workspace),
            data: BoardResult { board },
        })
    }

    async fn boards_create(
        &self,
        ctx: &mut CommandCtx,
        args: BoardsCreateArgs,
    ) -> Result<BoardResult, ApiError> {
        ctx.workspace_scope(&args.workspace, WorkspaceCapability::Write, None)
            .await?;
        let user = ctx.person()?;
        let name = definition_name(&args.name, "name")?;
        let id = BoardId::new(crate::db::new_id());
        let now = ctx.now();
        sqlx::query("INSERT INTO boards (id, workspace_id, name, created_by, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(id.as_str())
            .bind(args.workspace.as_str())
            .bind(&name)
            .bind(user.as_str())
            .bind(now)
            .bind(now)
            .execute(ctx.conn())
            .await?;
        ctx.record(
            &Topic::workspace(&args.workspace),
            "board.created",
            json!({ "id": id }),
        )
        .await?;
        Ok(BoardResult {
            board: load(ctx.conn(), &id, true, None).await?,
        })
    }

    async fn boards_update(
        &self,
        ctx: &mut CommandCtx,
        args: BoardsUpdateArgs,
    ) -> Result<BoardResult, ApiError> {
        let workspace = board_workspace(ctx.conn(), &args.board).await?;
        ctx.workspace_scope(
            &workspace,
            WorkspaceCapability::Write,
            args.expected_revision,
        )
        .await?;
        let name = definition_name(&args.name, "name")?;
        let existing: Vec<String> =
            sqlx::query_scalar("SELECT id FROM board_cards WHERE board_id = ?")
                .bind(args.board.as_str())
                .fetch_all(ctx.conn())
                .await?;
        let mut seen = std::collections::HashSet::new();
        for card in &args.cards {
            if !existing.iter().any(|id| id == card.id.as_str()) || !seen.insert(card.id.as_str()) {
                return Err(FieldErrors::one(
                    "cards",
                    FieldErrorCode::InvalidValue,
                    None,
                ));
            }
        }
        let titles = args
            .cards
            .iter()
            .map(|card| definition_name(&card.title, "cards"))
            .collect::<Result<Vec<_>, _>>()?;
        for id in existing.iter().filter(|id| !seen.contains(id.as_str())) {
            sqlx::query("DELETE FROM board_cards WHERE id = ?")
                .bind(id)
                .execute(ctx.conn())
                .await?;
        }
        for (position, (card, title)) in args.cards.iter().zip(titles).enumerate() {
            sqlx::query("UPDATE board_cards SET position = ?, title = ? WHERE id = ?")
                .bind(position as i64)
                .bind(title)
                .bind(card.id.as_str())
                .execute(ctx.conn())
                .await?;
        }
        let now = ctx.now();
        sqlx::query("UPDATE boards SET name = ?, updated_at = ? WHERE id = ?")
            .bind(&name)
            .bind(now)
            .bind(args.board.as_str())
            .execute(ctx.conn())
            .await?;
        ctx.record(
            &Topic::workspace(&workspace),
            "board.updated",
            json!({ "id": args.board }),
        )
        .await?;
        Ok(BoardResult {
            board: load(ctx.conn(), &args.board, true, None).await?,
        })
    }

    async fn boards_pin(
        &self,
        ctx: &mut CommandCtx,
        args: BoardsPinArgs,
    ) -> Result<BoardResult, ApiError> {
        let workspace = board_workspace(ctx.conn(), &args.board).await?;
        ctx.workspace_scope(&workspace, WorkspaceCapability::Write, None)
            .await?;
        let scope = ctx
            .site_scope(&args.state.site, Capability::SiteRead, None)
            .await?;
        if scope.workspace() != &workspace {
            return Err(FieldErrors::one(
                "state",
                FieldErrorCode::InvalidValue,
                None,
            ));
        }
        checked_state(&args.state, scope.site())?;
        let title = definition_name(&args.title, "title")?;
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM board_cards WHERE board_id = ?")
            .bind(args.board.as_str())
            .fetch_one(ctx.conn())
            .await?;
        if count as usize >= BOARD_CARDS_MAX {
            return Err(FieldErrors::one(
                "board",
                FieldErrorCode::TooMany,
                Some(BOARD_CARDS_MAX as u32),
            ));
        }
        sqlx::query(
            "INSERT INTO board_cards (id, board_id, position, title, site_id, state, exploration_id) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(crate::db::new_id())
        .bind(args.board.as_str())
        .bind(count)
        .bind(&title)
        .bind(scope.site().as_str())
        .bind(serde_json::to_string(&args.state).map_err(ApiError::internal)?)
        .bind(args.exploration.as_ref().map(ExplorationId::as_str))
        .execute(ctx.conn())
        .await?;
        sqlx::query("UPDATE boards SET updated_at = ? WHERE id = ?")
            .bind(ctx.now())
            .bind(args.board.as_str())
            .execute(ctx.conn())
            .await?;
        ctx.record(
            &Topic::workspace(&workspace),
            "board.pinned",
            json!({ "id": args.board }),
        )
        .await?;
        Ok(BoardResult {
            board: load(ctx.conn(), &args.board, true, None).await?,
        })
    }

    async fn boards_delete(
        &self,
        ctx: &mut CommandCtx,
        args: BoardArgs,
    ) -> Result<Deleted, ApiError> {
        let workspace = board_workspace(ctx.conn(), &args.board).await?;
        ctx.workspace_scope(&workspace, WorkspaceCapability::Write, None)
            .await?;
        sqlx::query("DELETE FROM boards WHERE id = ?")
            .bind(args.board.as_str())
            .execute(ctx.conn())
            .await?;
        ctx.record(
            &Topic::workspace(&workspace),
            "board.deleted",
            json!({ "id": args.board }),
        )
        .await?;
        Ok(Deleted { deleted: true })
    }
}

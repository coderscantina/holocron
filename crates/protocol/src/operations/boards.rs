//! Workspace boards: ordered cards, each a complete exploration state of a site in the
//! workspace. Every workspace member reads boards; viewers cannot change them. Reading a
//! card's data still needs `site.read` on its site through `analytics.explore`. API keys reach
//! boards only when they hold every site. A board share reads its own board with `boards.get`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::exploration::ExplorationState;
use crate::ids::{BoardId, CardId, ExplorationId, WorkspaceId};
use crate::operations::explorations::Deleted;

crate::domain! {
    pub trait BoardsHandlers;
    commands {
        /// Creates an empty board. Needs a workspace role above viewer.
        "boards.create" => boards_create (Person, key): BoardsCreateArgs => BoardResult;
        /// Renames a board and replaces its card list: reorder, retitle or remove cards by
        /// sending the list in the new order. Cards keep their IDs and states. Refused with
        /// `stale_revision` when `expected_revision` is older than the workspace topic.
        "boards.update" => boards_update (Person, key): BoardsUpdateArgs => BoardResult;
        /// Appends a card holding a complete exploration state of a site in the board's workspace.
        "boards.pin" => boards_pin (Person, key): BoardsPinArgs => BoardResult;
        "boards.delete" => boards_delete (Person, key): BoardArgs => Deleted;
    }
    queries {
        /// The workspace's boards by name.
        "boards.list" => boards_list (Person, key): BoardsListArgs => BoardList;
        "boards.get" => boards_get (Person, key, share_read): BoardArgs => BoardResult;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct Board {
    pub id: BoardId,
    pub workspace: WorkspaceId,
    pub name: String,
    pub cards: Vec<Card>,
    /// Whether the caller may change the board.
    pub can_edit: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct Card {
    pub id: CardId,
    pub title: String,
    /// Absent when the card is unavailable: its site is outside the caller's board share.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub state: Option<ExplorationState>,
    /// The saved exploration the card was pinned from, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub exploration: Option<ExplorationId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct BoardSummary {
    pub id: BoardId,
    pub name: String,
    pub cards: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct BoardList {
    pub boards: Vec<BoardSummary>,
    /// Whether the caller may create boards in the workspace.
    pub can_edit: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct BoardResult {
    pub board: Board,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct BoardsListArgs {
    pub workspace: WorkspaceId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct BoardArgs {
    pub board: BoardId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct BoardsCreateArgs {
    pub workspace: WorkspaceId,
    /// 1 to 100 characters after trimming.
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct BoardsUpdateArgs {
    pub board: BoardId,
    pub name: String,
    /// The board's cards in their new order, with new titles. Every ID must be a card of the
    /// board; a card left out is removed.
    pub cards: Vec<CardOrder>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expected_revision: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct CardOrder {
    pub id: CardId,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct BoardsPinArgs {
    pub board: BoardId,
    /// 1 to 100 characters after trimming.
    pub title: String,
    pub state: ExplorationState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub exploration: Option<ExplorationId>,
}

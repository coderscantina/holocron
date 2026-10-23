//! Restricted shares of an exploration or a board (CAP-SHARE-04 to 06).
//!
//! A share viewer sends `Authorization: Share <token>` and reaches only the operations the
//! registry marks for shares, on the share's sites, within its limits; everything else answers
//! `not_found`. A request that widens the share's range, filter, measures or fields answers
//! `share_scope_exceeded`. The current and comparison ranges together must fit
//! `range_max_days` (decision 0007). Scope is resolved again on every read, export and live
//! frame, so a revoked or expired share stops at once.
//!
//! A share keeps a copy of the state it was created from, so it survives changes to, or the
//! deletion of, the saved exploration and its author.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::envelope::Empty;
use crate::exploration::{DimensionField, ExplorationState, Group, NamedMeasure, PropertyField};
use crate::ids::{BoardId, ExplorationId, ShareId, SiteId, UserId};

crate::domain! {
    pub trait SharingHandlers;
    commands {
        /// Creates a share and discloses its viewer token once. Exploration shares need
        /// `site.share` on the state's site, board shares `workspace.write` and `site.share` on
        /// every listed site. `export` needs `site.export`, `recordings` needs
        /// `site.recordings.read`: a share never grants more than its creator holds. The limits
        /// must not be wider than the shared state needs. `expires_at` at most 366 days ahead.
        /// Board shares grant no recordings, and their viewers explore only the board's cards
        /// with another range or page.
        "sharing.create" => sharing_create (Person, secret): SharingCreateArgs => ShareCreated;
        /// Changes the name, actions, lifetime and, for exploration shares, the limits. Same
        /// capabilities as `sharing.create`. The token stays. A revoked share cannot change.
        "sharing.update" => sharing_update (Person): SharingUpdateArgs => ShareResult;
        /// Revokes a share at once: viewers get `not_found` and their subscriptions end. Needs
        /// `site.share` (exploration) or `workspace.write` (board).
        "sharing.revoke" => sharing_revoke (Person): ShareArgs => ShareResult;
    }
    queries {
        /// Shares of one site (`site`) or one board (`board`), exactly one of the two, newest
        /// first, revoked and expired ones included. Same capabilities as `sharing.revoke`.
        "sharing.list" => sharing_list (Person): SharingListArgs => ShareList;
        /// What the caller's share token opens. Without a token, and for unknown, revoked and
        /// expired tokens, the same `not_found`.
        "sharing.resolve" => sharing_resolve (Anonymous, share_read): Empty => SharedView;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ShareTarget {
    /// One site's exploration, opened at `state` and bounded by `limits`.
    Exploration {
        state: Box<ExplorationState>,
        /// The saved exploration the state was copied from, if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        exploration: Option<ExplorationId>,
        limits: ExplorationLimits,
    },
    /// A board whose cards are readable on the listed sites only. Cards of other sites answer
    /// as unavailable, the same as a card whose site is missing.
    Board { board: BoardId, sites: Vec<SiteId> },
}

/// What a viewer of an exploration share may change. Anything outside is `share_scope_exceeded`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct ExplorationLimits {
    pub measures: Vec<NamedMeasure>,
    pub dimensions: Vec<DimensionField>,
    /// Custom properties usable as dimensions, filters, measures and value suggestions.
    pub properties: Vec<PropertyField>,
    /// Longest readable span in days, current and comparison range together. 1 to 366.
    pub range_max_days: u16,
    /// Every viewer state must contain this group in an AND at its root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pinned_filter: Option<Group>,
    /// Whether viewers may add conditions of granted fields next to the pinned filter.
    pub narrowing_allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct Share {
    pub id: ShareId,
    /// 1 to 100 characters after trimming.
    pub name: String,
    pub target: ShareTarget,
    /// Viewers may export what they can read.
    pub export: bool,
    /// Viewers may list and play recordings inside the share's sites, range and pinned filter.
    pub recordings: bool,
    pub expires_at: i64,
    /// The first characters of the token, to tell links apart.
    pub prefix: String,
    /// Absent once the author's account is deleted ("Deleted user").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub created_by: Option<UserId>,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub revoked_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SharingCreateArgs {
    pub name: String,
    pub target: ShareTarget,
    pub export: bool,
    pub recordings: bool,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ShareCreated {
    pub share: Share,
    /// The viewer token. The viewer link is `/share/<token>`. `null` in a replay.
    pub secret: Option<String>,
    pub secret_withheld: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SharingUpdateArgs {
    pub share: ShareId,
    pub name: String,
    pub export: bool,
    pub recordings: bool,
    pub expires_at: i64,
    /// New limits of an exploration share. Must be absent for board shares.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub limits: Option<ExplorationLimits>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expected_revision: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ShareArgs {
    pub share: ShareId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ShareResult {
    pub share: Share,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SharingListArgs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub site: Option<SiteId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub board: Option<BoardId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ShareList {
    pub shares: Vec<Share>,
}

/// The viewer's side of a share: no author, no token prefix.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SharedView {
    pub name: String,
    pub target: ShareTarget,
    pub export: bool,
    pub recordings: bool,
    pub expires_at: i64,
    /// The sites the share reaches, for headers and timezones.
    pub sites: Vec<SharedSite>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SharedSite {
    pub id: SiteId,
    pub name: String,
    pub domain: String,
    pub timezone: String,
}

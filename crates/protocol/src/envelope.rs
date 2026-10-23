//! The fields every command and query answer shares with its operation-specific result.
//!
//! A command body is `CommandKey & Args`; its answer is `CommandMeta & Result`. A query answer is
//! `SnapshotMeta & Result`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct CommandKey {
    /// Repeating a key repeats the same intent: the stored answer comes back without running
    /// the command again. 1 to 128 characters, kept for 24 hours.
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct CommandMeta {
    /// Sequence of the last topic event the command recorded, or the installation head when it
    /// recorded none.
    pub revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct SnapshotMeta {
    /// Head of the snapshot's topic, read in the same transaction as the data. Pass it as
    /// `expected_revision` to a command that changes the same resource.
    pub revision: i64,
    /// `<topic>@<revision>`. Opaque to the browser.
    pub cursor: String,
}

/// Arguments of operations that take none.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct Empty {}

/// A named, ordered log of state events: `installation`, `site:<id>`, `workspace:<id>`,
/// `user:<id>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Topic(String);

impl Topic {
    pub fn installation() -> Self {
        Self("installation".into())
    }

    pub fn site(site: &crate::ids::SiteId) -> Self {
        Self(format!("site:{site}"))
    }

    pub fn workspace(workspace: &crate::ids::WorkspaceId) -> Self {
        Self(format!("workspace:{workspace}"))
    }

    pub fn user(user: &crate::ids::UserId) -> Self {
        Self(format!("user:{user}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn cursor(&self, revision: i64) -> String {
        format!("{}@{revision}", self.0)
    }
}

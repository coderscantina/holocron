//! Saved goals of a site (0003 Goals). Every edit of a goal's definition is a new version;
//! earlier versions stay readable, so a report that copied one keeps its meaning and can name
//! where it came from. Reports store the definition inline with `ref {id, version}`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::exploration::GoalDefinition;
use crate::ids::{GoalId, SiteId};

crate::domain! {
    pub trait GoalsHandlers;
    commands {
        /// Creates a goal at version 1, or with `goal` saves its next version. A save that
        /// changes only the name renames the goal without a new version. Names are unique
        /// per site among active goals after case folding. Needs `site.definitions.write`.
        "goals.save" => goals_save (Person, key): GoalsSaveArgs => SavedGoalResult;
        /// Archives a goal: it leaves the list, and its versions stay readable for reports
        /// that reference them. Needs `site.definitions.write`.
        "goals.archive" => goals_archive (Person, key): GoalsGetArgs => SavedGoalResult;
    }
    queries {
        /// The site's active goals by name, each at its latest version. Needs `site.read`.
        "goals.list" => goals_list (Person, key): GoalsListArgs => SavedGoalList;
        /// One version of a goal, archived ones too; the latest without `version`.
        /// Needs `site.read`.
        "goals.get" => goals_get (Person, key): GoalsGetArgs => SavedGoalResult;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SavedGoal {
    pub id: GoalId,
    pub site: SiteId,
    pub name: String,
    /// The version `definition` belongs to.
    pub version: u32,
    /// The goal's newest version.
    pub latest_version: u32,
    /// Without `ref`; a report copying it adds `ref {id, version}`.
    pub definition: GoalDefinition,
    pub archived: bool,
    /// When this version was saved, Unix milliseconds.
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SavedGoalResult {
    pub goal: SavedGoal,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SavedGoalList {
    pub goals: Vec<SavedGoal>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct GoalsListArgs {
    pub site: SiteId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct GoalsGetArgs {
    pub site: SiteId,
    pub goal: GoalId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub version: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct GoalsSaveArgs {
    pub site: SiteId,
    /// 1 to 100 characters after trimming.
    pub name: String,
    /// Its `ref` is ignored.
    pub definition: GoalDefinition,
    /// Saves the next version of this goal instead of creating one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub goal: Option<GoalId>,
}

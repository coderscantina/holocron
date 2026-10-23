//! The workspaces and sites a person may choose in the header.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::access::{Role, WorkspaceKind};
use crate::envelope::Empty;
use crate::ids::{SiteId, WorkspaceId};

crate::domain! {
    pub trait WorkspacesHandlers;
    commands {}
    queries {
        /// Workspaces the caller is a member of, each with its sites, ordered by name. This is
        /// the only source of header context options.
        "workspaces.list" => workspaces_list (Person): Empty => WorkspaceList;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct WorkspaceList {
    pub workspaces: Vec<WorkspaceSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct WorkspaceSummary {
    pub id: WorkspaceId,
    pub name: String,
    pub kind: WorkspaceKind,
    pub role: Role,
    pub sites: Vec<SiteSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SiteSummary {
    pub id: SiteId,
    pub name: String,
    pub domain: String,
}

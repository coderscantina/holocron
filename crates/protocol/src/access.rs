//! Roles and capabilities from `fixtures/access/role-mapping.json`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A person's role in one workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Owner,
    Admin,
    Member,
    Viewer,
}

/// What a site scope allows. The server derives the set from the role; the browser only displays it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
pub enum Capability {
    #[serde(rename = "site.read")]
    SiteRead,
    #[serde(rename = "site.export")]
    SiteExport,
    #[serde(rename = "site.recordings.read")]
    SiteRecordingsRead,
    #[serde(rename = "site.definitions.write")]
    SiteDefinitionsWrite,
    #[serde(rename = "site.configure")]
    SiteConfigure,
    #[serde(rename = "site.share")]
    SiteShare,
    #[serde(rename = "site.delete")]
    SiteDelete,
    #[serde(rename = "site.transfer")]
    SiteTransfer,
}

/// What a workspace membership allows besides site capabilities (`role-mapping.json`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
pub enum WorkspaceCapability {
    /// See the workspace, its members and boards.
    #[serde(rename = "workspace.read")]
    Read,
    /// Create sites and change boards.
    #[serde(rename = "workspace.write")]
    Write,
    /// Rename the team, invite, change roles, remove members, set the team 2FA requirement.
    #[serde(rename = "team.manage")]
    Manage,
}

impl Role {
    /// Owner 3, admin 2, member 1, viewer 0. An actor must outrank a member to change or remove
    /// them and may not grant a role at or above its own (R-RANK).
    pub fn rank(self) -> u8 {
        match self {
            Role::Owner => 3,
            Role::Admin => 2,
            Role::Member => 1,
            Role::Viewer => 0,
        }
    }

    pub fn workspace_capabilities(self) -> &'static [WorkspaceCapability] {
        use WorkspaceCapability::*;
        match self {
            Role::Viewer => &[Read],
            Role::Member => &[Read, Write],
            Role::Admin | Role::Owner => &[Read, Write, Manage],
        }
    }

    /// The site capabilities this role grants, in declaration order.
    pub fn site_capabilities(self) -> &'static [Capability] {
        use Capability::*;
        match self {
            Role::Viewer => &[SiteRead, SiteExport, SiteRecordingsRead],
            Role::Member => &[
                SiteRead,
                SiteExport,
                SiteRecordingsRead,
                SiteDefinitionsWrite,
                SiteConfigure,
                SiteShare,
            ],
            Role::Admin | Role::Owner => &[
                SiteRead,
                SiteExport,
                SiteRecordingsRead,
                SiteDefinitionsWrite,
                SiteConfigure,
                SiteShare,
                SiteDelete,
                SiteTransfer,
            ],
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Admin => "admin",
            Role::Member => "member",
            Role::Viewer => "viewer",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [Role::Owner, Role::Admin, Role::Member, Role::Viewer]
            .into_iter()
            .find(|role| role.as_str() == value)
    }
}

/// Installation-wide role. `install_admin` manages the installation and reads no site data
/// unless it is also a workspace member (decision 0007).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InstallRole {
    User,
    InstallAdmin,
}

impl InstallRole {
    pub fn as_str(self) -> &'static str {
        match self {
            InstallRole::User => "user",
            InstallRole::InstallAdmin => "install_admin",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [InstallRole::User, InstallRole::InstallAdmin]
            .into_iter()
            .find(|role| role.as_str() == value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceKind {
    Personal,
    Team,
}

impl WorkspaceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            WorkspaceKind::Personal => "personal",
            WorkspaceKind::Team => "team",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [WorkspaceKind::Personal, WorkspaceKind::Team]
            .into_iter()
            .find(|kind| kind.as_str() == value)
    }
}

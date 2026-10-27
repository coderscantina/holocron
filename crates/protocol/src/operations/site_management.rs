//! Creating, listing, deleting, resetting and transferring sites (CAP-ACCESS-10). Kept apart from
//! `sites` so site settings and site lifecycle have separate handler traits.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::access::Role;
use crate::envelope::Empty;
use crate::ids::{SiteId, UserId, WorkspaceId};
use crate::operations::explorations::Deleted;
use crate::operations::sites::{SiteDetail, SitesGetArgs};

crate::domain! {
    pub trait SiteManagementHandlers;
    commands {
        /// Creates a site in a workspace. Needs `workspace.write`. Name and domain follow
        /// `sites.update`; the timezone starts the site's first session policy.
        "sites.create" => sites_create (Person): SitesCreateArgs => SiteDetail;
        /// Deletes a site with its analytical data, recordings, definitions, shares and key
        /// grants, in both engines. Needs `site.delete` (admin and owner, decision 0007) and
        /// reauthentication.
        "sites.delete" => sites_delete (Person, reauth): SitesGetArgs => Deleted;
        /// Removes every event, session and recording of a site and keeps its settings,
        /// definitions and shares. Needs `site.delete` and reauthentication.
        "sites.reset" => sites_reset (Person, reauth): SitesGetArgs => SiteReset;
        /// Moves a site to another workspace. Needs `site.transfer` on the site and
        /// `workspace.write` on the target team; a hidden target is `not_found`. Moving to a
        /// user's personal workspace is owner only (R-OWNER-ONLY). Needs reauthentication.
        /// Shares and key grants of the site end.
        "sites.transfer" => sites_transfer (Person, reauth): SitesTransferArgs => SiteDetail;
    }
    queries {
        /// Sites the caller is a member for, by workspace and name. Install admins get every
        /// site's metadata (name, domain, workspace), with `role` absent where they are not
        /// members; no analytics (decision 0007). An API key sees only the sites it reaches.
        "sites.list" => sites_list (Person, key): Empty => SiteList;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SitesCreateArgs {
    pub workspace: WorkspaceId,
    pub name: String,
    pub domain: String,
    /// IANA timezone name.
    pub timezone: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SiteReset {
    pub site: SiteId,
    pub reset_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SitesTransferArgs {
    pub site: SiteId,
    pub to: TransferTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(tag = "direction", rename_all = "snake_case")]
pub enum TransferTarget {
    ToTeam {
        team: WorkspaceId,
    },
    /// The user's personal workspace.
    ToUser {
        user: UserId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SiteList {
    pub sites: Vec<SiteListItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SiteListItem {
    pub id: SiteId,
    pub name: String,
    pub domain: String,
    pub workspace: WorkspaceId,
    pub workspace_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub role: Option<Role>,
}

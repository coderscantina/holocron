//! One site's settings, read and changed through an authorized site scope.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::access::Capability;
use crate::ids::{SiteId, WorkspaceId};
use crate::retention::Retention;

crate::domain! {
    pub trait SitesHandlers;
    commands {
        /// Renames a site or changes its domain. Needs `site.configure`. Refused with
        /// `stale_revision` when `expected_revision` is older than the site topic.
        "sites.update" => sites_update (Person, key): SitesUpdateArgs => SitesUpdateResult;
        /// Changes the timezone or inactivity window as a new session policy version. Needs
        /// `site.configure`. A timezone change rotates identity at `effective_at` and may not
        /// take effect in the past; a window change effective in the past recalculates from
        /// there (0002). An unchanged policy records nothing.
        "sites.updateSessionPolicy" => sites_update_session_policy (Person, key): SitesUpdateSessionPolicyArgs => SitesUpdateResult;
        /// Turns persistent identified analytics on or off and bumps the identity policy
        /// version. Needs `site.configure`. Turning it off revokes every consent at once
        /// (0001). An unchanged setting records nothing.
        "sites.updateIdentityPolicy" => sites_update_identity_policy (Person, key): SitesUpdateIdentityPolicyArgs => IdentityPolicy;
        /// Sets how long the site keeps analytical events, or follows the installation default
        /// without `retention` (0016). Needs `site.delete`. Shortening needs reauthentication;
        /// queries hide what lies before the new cutoff at once and the hourly retention job
        /// removes it. An unchanged setting records nothing.
        "sites.updateRetention" => sites_update_retention (Person, reauth_by_handler): SitesUpdateRetentionArgs => SitesUpdateResult;
    }
    queries {
        /// One site with the caller's capabilities on it. A hidden site answers like a missing one.
        "sites.get" => sites_get (Person, key): SitesGetArgs => SiteDetail;
        /// Whether the site links consented visitors to persistent profiles. Needs `site.configure`.
        "sites.identityPolicy" => sites_identity_policy (Person, key): SitesGetArgs => IdentityPolicy;
        /// What `sites.updateRetention` with the same arguments would remove. Needs `site.delete`.
        "sites.retentionPreview" => sites_retention_preview (Person): SitesUpdateRetentionArgs => RetentionPreview;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SitesGetArgs {
    pub site: SiteId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SiteDetail {
    pub site: Site,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct Site {
    pub id: SiteId,
    pub workspace: WorkspaceId,
    pub name: String,
    pub domain: String,
    /// IANA timezone the site reports in, from its current session policy.
    pub timezone: String,
    /// A gap of at least this many minutes between events starts a new session.
    pub session_window_minutes: u16,
    /// Counts timezone and window changes, starting at 1.
    pub session_policy_version: i32,
    /// How long the site keeps analytical events: its own setting or the installation default.
    pub retention: Retention,
    /// The site's own setting. Absent when it follows the installation default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub retention_override: Option<Retention>,
    /// What the caller may do with this site.
    pub capabilities: Vec<Capability>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SitesUpdateArgs {
    pub site: SiteId,
    /// 1 to 100 characters after trimming.
    pub name: String,
    /// Host name without scheme or path, for example `roadhouse.test`.
    pub domain: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expected_revision: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SitesUpdateResult {
    pub site: Site,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SitesUpdateSessionPolicyArgs {
    pub site: SiteId,
    /// IANA timezone name, for example `Europe/Vienna`.
    pub timezone: String,
    /// 1 to 1440.
    pub window_minutes: u16,
    /// First event time the new version applies to, Unix milliseconds. Defaults to now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub effective_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expected_revision: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SitesUpdateIdentityPolicyArgs {
    pub site: SiteId,
    pub persistent_identity: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expected_revision: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct IdentityPolicy {
    /// Consented visitors are linked to persistent profiles. Without it every `consent`
    /// signal is ignored and counted.
    pub persistent_identity: bool,
    /// Counts changes of the identity policy, starting at 1.
    pub identity_policy_version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SitesUpdateRetentionArgs {
    pub site: SiteId,
    /// The site's own setting. Absent follows the installation default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub retention: Option<Retention>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expected_revision: Option<i64>,
}

/// What a retention change would remove.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct RetentionPreview {
    /// The retention that would apply.
    pub retention: Retention,
    /// The new retention cutoff, Unix milliseconds, in the source's own timezone; for several
    /// sources the earliest. Absent for `forever` or when nothing changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cutoff: Option<i64>,
    /// Stored events the change would remove.
    pub events_removed: u64,
    /// Sources whose retention would change.
    pub sources: u32,
}

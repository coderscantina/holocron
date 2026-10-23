//! 2FA requirement policy at installation, team and user level (CAP-ACCESS-04). A person must use
//! 2FA when any policy that covers them requires it; unenrolled people then get enrollment-only
//! sessions at their next request, without a grace period (decision 0007).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::envelope::Empty;
use crate::ids::{UserId, WorkspaceId};

crate::domain! {
    pub trait SecurityHandlers;
    commands {
        /// Turns a 2FA requirement on or off. Installation and user scope need `install.admin`,
        /// team scope needs `team.manage` on the team (a hidden team is `not_found`). Needs
        /// reauthentication. An unchanged setting records nothing.
        "security.updatePolicy" => security_update_policy (Person, reauth): SecurityUpdatePolicyArgs => SecurityPolicy;
    }
    queries {
        /// The requirements the caller may see: the installation's, and each team where the
        /// caller holds `team.manage` (every team for install admins). User-level requirements
        /// are in `users.list`.
        "security.policy" => security_policy (Person): Empty => SecurityPolicy;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SecurityPolicy {
    pub installation_two_factor_required: bool,
    /// Whether the caller may change installation and user requirements.
    pub can_manage_installation: bool,
    pub teams: Vec<TeamPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct TeamPolicy {
    pub workspace: WorkspaceId,
    pub name: String,
    pub two_factor_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SecurityUpdatePolicyArgs {
    pub target: PolicyTarget,
    pub two_factor_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum PolicyTarget {
    Installation,
    Team { workspace: WorkspaceId },
    User { user: UserId },
}

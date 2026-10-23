//! Installation administration of accounts. Needs `install.admin`; everyone else gets
//! `forbidden`. An install admin reads no site data through these operations (decision 0007).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::access::{InstallRole, Role, WorkspaceKind};
use crate::envelope::Empty;
use crate::ids::{UserId, WorkspaceId};

crate::domain! {
    pub trait UsersHandlers;
    commands {
        /// Creates an account with a generated password, disclosed once for the admin to hand
        /// over. There is no self sign-up and no default password (R-BOOT). A taken email is
        /// `invalid_arguments` on `email`.
        "users.create" => users_create (Person, secret): UsersCreateArgs => UserCreated;
        /// Changes name, email or installation role. Demoting the last install admin is
        /// `last_install_admin`.
        "users.update" => users_update (Person): UsersUpdateArgs => UserResult;
        /// The recovery path without email: sets a new generated password (disclosed once)
        /// and/or removes the second factor, and ends the user's sessions. Needs reauthentication.
        "users.resetCredentials" => users_reset_credentials (Person, reauth, secret): UsersResetCredentialsArgs => CredentialsReset;
        /// Erases an account as decision 0007 User erasure scope lists. Needs reauthentication.
        /// Refused with `user_owns_resources` (naming them in `owned`) while the user is the
        /// only owner of a workspace or owns personal sites, and with `last_install_admin` for
        /// the last install admin. Leaves a tombstone of ID and deletion time.
        "users.delete" => users_delete (Person, reauth): UserArgs => UserDeleted;
    }
    queries {
        /// Every account by email.
        "users.list" => users_list (Person): Empty => UserList;
        /// One account with its workspace memberships.
        "users.get" => users_get (Person): UserArgs => UserDetail;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct UserSummary {
    pub id: UserId,
    pub email: String,
    pub name: String,
    pub install_role: InstallRole,
    pub two_factor_enrolled: bool,
    /// The user-level 2FA requirement (`security.updatePolicy` with scope `user`).
    pub two_factor_required: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct UserMembership {
    pub workspace: WorkspaceId,
    pub name: String,
    pub kind: WorkspaceKind,
    pub role: Role,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct UserList {
    pub users: Vec<UserSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct UserArgs {
    pub user: UserId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct UserDetail {
    pub user: UserSummary,
    pub memberships: Vec<UserMembership>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct UserResult {
    pub user: UserSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct UsersCreateArgs {
    pub email: String,
    /// 1 to 100 characters after trimming.
    pub name: String,
    pub install_role: InstallRole,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct UserCreated {
    pub user: UserSummary,
    /// The generated password. `null` in a replayed answer.
    pub secret: Option<String>,
    pub secret_withheld: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct UsersUpdateArgs {
    pub user: UserId,
    pub name: String,
    pub email: String,
    pub install_role: InstallRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expected_revision: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct UsersResetCredentialsArgs {
    pub user: UserId,
    /// Replace the password with a generated one.
    pub password: bool,
    /// Remove the second factor and its recovery codes.
    pub two_factor: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct CredentialsReset {
    pub user: UserSummary,
    /// The generated password when `password` was set. `null` otherwise and in a replay.
    pub secret: Option<String>,
    pub secret_withheld: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct UserDeleted {
    pub user: UserId,
    pub deleted_at: i64,
}

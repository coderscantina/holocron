//! The signed-in person's own profile, password and preferences. Needs `account.self`, which every
//! person holds; API keys and share viewers never reach these operations.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::envelope::Empty;
use crate::exploration::Range;
use crate::operations::auth::Account;

crate::domain! {
    pub trait AccountHandlers;
    commands {
        /// Changes the caller's name and email. Emails are unique after trimming and lower
        /// casing; a taken email is `invalid_arguments` on `email`.
        "account.update" => account_update (Person): AccountUpdateArgs => AccountResult;
        /// Sets a new password and ends every other session of the caller (R-AUTH). Needs
        /// reauthentication. 12 to 256 characters.
        "account.changePassword" => account_change_password (Person, reauth): ChangePasswordArgs => Empty;
        /// Replaces the caller's preferences. Absent fields fall back to the installation default.
        "account.updatePreferences" => account_update_preferences (Person): Preferences => Preferences;
    }
    queries {
        /// The caller's account. Reachable from an enrollment-only session.
        "account.get" => account_get (Person, enrollment): Empty => AccountResult;
        "account.preferences" => account_preferences (Person): Empty => Preferences;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct AccountResult {
    pub account: Account,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct AccountUpdateArgs {
    /// 1 to 100 characters after trimming.
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ChangePasswordArgs {
    pub new_password: String,
}

/// Per-person display settings. The browser applies them; the server only stores them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct Preferences {
    /// IANA timezone new explorations start in, for example `Europe/Vienna`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub timezone: Option<String>,
    /// Range new explorations start with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub default_range: Option<Range>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub locale: Option<Locale>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub theme: Option<Theme>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub accent: Option<Accent>,
}

/// Languages the web app ships catalogs for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Locale {
    En,
    De,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    System,
    Light,
    Dark,
}

/// Accent colour presets the web app ships.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Accent {
    Indigo,
    Blue,
    Violet,
    Rose,
    Orange,
    Amber,
    Emerald,
    Cyan,
}

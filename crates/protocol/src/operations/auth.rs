//! Sign-in with an optional second factor, sign-out, reauthentication and the current session.
//!
//! Session and challenge tokens travel only in HttpOnly cookies, never in a body, so stored
//! idempotent answers hold no credential (decision 0009, Session commands).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::access::InstallRole;
use crate::envelope::Empty;
use crate::ids::UserId;

crate::domain! {
    pub trait AuthHandlers;
    commands {
        /// Checks email and password. Wrong password and unknown email answer the same
        /// `invalid_credentials`. Without 2FA it sets the session cookie and answers
        /// `signed_in`. An enrolled account answers `second_factor_required` and sets a
        /// challenge cookie for `auth.verifySecondFactor`; the challenge is never a session. An
        /// account that must use 2FA but has not enrolled gets an enrollment-only session
        /// (decision 0007). 10 attempts per client address per minute (`rate_limited`). A
        /// replayed key returns the stored answer without a new session or challenge.
        "auth.login" => auth_login (Anonymous): LoginArgs => LoginResult;
        /// Finishes a login with a time-based code or an unused recovery code and turns the
        /// challenge cookie into a session cookie. A wrong or used code is
        /// `second_factor_invalid`; after 5 wrong codes, or 5 minutes, the challenge ends and
        /// answers `unauthenticated`.
        "auth.verifySecondFactor" => auth_verify_second_factor (Anonymous): SecondFactorProof => LoginResult;
        /// Ends the current session and clears its cookie.
        "auth.logout" => auth_logout (Person, enrollment): Empty => Empty;
        /// Checks the password, and the second factor when enrolled, and marks the current
        /// session for 10 minutes so sensitive commands pass (decision 0007). A wrong password
        /// is `invalid_credentials`, a missing second factor `second_factor_required`, a wrong
        /// one `second_factor_invalid`; none of them ends the session. Shares the login limit.
        "auth.reauthenticate" => auth_reauthenticate (Person): ReauthenticateArgs => Reauthenticated;
        /// Ends every other session of the caller. The current one stays.
        "auth.revokeSessions" => auth_revoke_sessions (Person): Empty => SessionsRevoked;
    }
    queries {
        /// The signed-in account, or `null`. Answers signed-out callers and share viewers too,
        /// never 401. API keys cannot hold it.
        "auth.session" => auth_session (Anonymous): Empty => SessionState;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct LoginArgs {
    pub email: String,
    pub password: String,
}

/// The outcome of a correct password or second factor.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LoginResult {
    /// A full session cookie is set.
    SignedIn { account: Account },
    /// A challenge cookie is set; call `auth.verifySecondFactor` next.
    SecondFactorRequired,
    /// An enrollment-only session cookie is set: only `twoFactor.begin`, `twoFactor.confirm`,
    /// `account.get` and `auth.logout` answer until enrollment is confirmed.
    EnrollmentRequired { account: Account },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SecondFactorProof {
    /// The current 6-digit code of the authenticator app.
    Totp { code: String },
    /// One unused recovery code. Spaces and dashes are ignored, letters are not case sensitive.
    RecoveryCode { code: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ReauthenticateArgs {
    pub password: String,
    /// Needed when the account is enrolled in 2FA.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub second_factor: Option<SecondFactorProof>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct Reauthenticated {
    /// Sensitive commands pass until this instant, Unix milliseconds.
    pub reauthenticated_until: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SessionsRevoked {
    /// Sessions ended, not counting the current one.
    pub revoked: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SessionState {
    pub account: Option<Account>,
    /// The session may only enroll a second factor (decision 0007). False when signed out.
    pub enrollment_only: bool,
    /// Sensitive commands pass without a password prompt until this instant, Unix
    /// milliseconds. Absent when the session has not reauthenticated or the window has passed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reauthenticated_until: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct Account {
    pub id: UserId,
    pub email: String,
    pub name: String,
    pub install_role: InstallRole,
}

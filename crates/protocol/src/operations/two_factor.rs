//! Time-based one-time codes (RFC 6238) and recovery codes for the caller's own account.
//! Needs `account.self`. Secrets are encrypted at rest and recovery codes stored as hashes
//! (decision 0012); both are disclosed once and withheld from stored idempotent answers.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::envelope::Empty;

crate::domain! {
    pub trait TwoFactorHandlers;
    commands {
        /// Starts enrollment, or reenrollment while enrolled: creates a pending secret and
        /// discloses it once. The active factor, if any, keeps working until `twoFactor.confirm`.
        /// Calling it again replaces the pending secret. Reachable from an enrollment-only session.
        /// Reenrollment needs reauthentication; a first enrollment does not.
        "twoFactor.begin" => two_factor_begin (Person, enrollment, secret, reauth_by_handler): Empty => TwoFactorEnrollment;
        /// Confirms the pending secret with a current code, makes it the active factor and
        /// discloses a fresh set of 10 recovery codes once. An enrollment-only session becomes a
        /// full session. A wrong code is `second_factor_invalid`; no pending secret is
        /// `invalid_arguments` on `code`. Replacing a confirmed factor needs reauthentication.
        "twoFactor.confirm" => two_factor_confirm (Person, enrollment, secret, reauth_by_handler): TwoFactorConfirmArgs => RecoveryCodes;
        /// Removes the factor and its recovery codes. Needs reauthentication. When a policy
        /// requires 2FA, the caller's sessions become enrollment-only at their next request.
        "twoFactor.disable" => two_factor_disable (Person, reauth): Empty => TwoFactorStatus;
        /// Replaces every recovery code with 10 new ones, disclosed once. Needs reauthentication.
        "twoFactor.regenerateRecoveryCodes" => two_factor_regenerate_recovery_codes (Person, reauth, secret): Empty => RecoveryCodes;
    }
    queries {
        "twoFactor.status" => two_factor_status (Person): Empty => TwoFactorStatus;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct TwoFactorStatus {
    pub enrolled: bool,
    /// When the active factor was confirmed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub enrolled_at: Option<i64>,
    /// An installation, team or user policy requires 2FA for the caller.
    pub required: bool,
    /// Unused recovery codes left.
    pub recovery_codes_left: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct TwoFactorEnrollment {
    /// `null` in a replayed answer.
    pub secret: Option<TotpSecret>,
    /// True when this answer is a replay and the secret was withheld.
    pub secret_withheld: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct TotpSecret {
    /// The shared key in base32 without padding, for manual entry.
    pub key: String,
    /// `otpauth://totp/...` URI for a QR code, with issuer, account email, SHA1, 6 digits, 30 s.
    pub uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct TwoFactorConfirmArgs {
    /// The current 6-digit code for the pending secret.
    pub code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct RecoveryCodes {
    /// 10 codes of the form `xxxx-xxxx-xxxx-xxxx`. `null` in a replayed answer.
    pub secret: Option<Vec<String>>,
    /// True when this answer is a replay and the codes were withheld.
    pub secret_withheld: bool,
}

//! The closed set of API error codes and the error body every non-2xx answer carries.
//!
//! Screens branch on `error_code`, never on `message`. Access codes follow the decision order in
//! `fixtures/access/README.md`; exploration codes follow decision 0004.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ids::{SiteId, WorkspaceId};

macro_rules! error_codes {
    ($( $(#[doc = $doc:literal])* $variant:ident = $code:literal, $status:literal; )*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, JsonSchema)]
        pub enum ErrorCode {
            $( $(#[doc = $doc])* #[serde(rename = $code)] $variant, )*
        }

        impl ErrorCode {
            /// Every code, in declaration order.
            pub const ALL: &'static [ErrorCode] = &[$(ErrorCode::$variant),*];

            pub fn as_str(self) -> &'static str {
                match self { $(ErrorCode::$variant => $code,)* }
            }

            /// The HTTP status the code is always sent with.
            pub fn status(self) -> u16 {
                match self { $(ErrorCode::$variant => $status,)* }
            }
        }
    };
}

error_codes! {
    /// No valid credential. Sent whether or not the resource exists.
    Unauthenticated = "unauthenticated", 401;
    /// Wrong password or unknown account. Both answer identically.
    InvalidCredentials = "invalid_credentials", 401;
    /// The password was correct; a second factor is needed next.
    SecondFactorRequired = "second_factor_required", 401;
    SecondFactorInvalid = "second_factor_invalid", 401;
    /// The session may only enroll a second factor (decision 0007).
    SecondFactorEnrollmentRequired = "second_factor_enrollment_required", 403;
    /// A sensitive command needs a password check within the last 10 minutes.
    ReauthenticationRequired = "reauthentication_required", 403;
    /// Missing, hidden and unknown resources and operations all answer with this code and the same body.
    NotFound = "not_found", 404;
    /// The resource is visible but the role lacks the capability.
    Forbidden = "forbidden", 403;
    /// An API key does not hold this operation.
    OperationNotGranted = "operation_not_granted", 403;
    GrantExceedsPrincipal = "grant_exceeds_principal", 403;
    /// The request widens a shared exploration.
    ShareScopeExceeded = "share_scope_exceeded", 403;
    /// A mutation without the request marker or from another origin.
    OriginRejected = "origin_rejected", 403;
    /// `expected_revision` is older than the resource. `current` carries the head.
    StaleRevision = "stale_revision", 409;
    /// The idempotency key was used for another operation or other arguments.
    IdempotencyConflict = "idempotency_conflict", 409;
    /// `users.delete` while the user is the only owner of a workspace or owns personal sites.
    /// `owned` lists them.
    UserOwnsResources = "user_owns_resources", 409;
    LastInstallAdmin = "last_install_admin", 409;
    /// Saving this replay would put saved replays over their 5 GiB part of the recording cap.
    SavedReplayBudgetExceeded = "saved_replay_budget_exceeded", 409;
    /// The request does not parse: bad JSON, wrong types, missing idempotency key.
    MalformedRequest = "malformed_request", 400;
    /// Arguments parse but fail validation. `errors` names each field.
    InvalidArguments = "invalid_arguments", 422;
    /// Two conditions in one exploration state share a predicate ID.
    DuplicatePredicateId = "duplicate_predicate_id", 422;
    FilterTooDeep = "filter_too_deep", 422;
    TooManyConditions = "too_many_conditions", 422;
    PageOutOfRange = "page_out_of_range", 422;
    ReportMissing = "report_missing", 422;
    ReportInvalid = "report_invalid", 422;
    ReportFieldUnsupported = "report_field_unsupported", 422;
    ReportVersionUnsupported = "report_version_unsupported", 422;
    ReportTooLarge = "report_too_large", 422;
    /// The comparison cannot be computed for this state: a `typical_weekday` baseline with a
    /// money measure, a percentile, identified people, a report kind other than goals, or a
    /// range over 31 days (decision 0015).
    CompareUnsupported = "compare_unsupported", 422;
    /// The body or query string exceeds its byte limit.
    PayloadTooLarge = "payload_too_large", 413;
    RateLimited = "rate_limited", 429;
    /// An analytical query ran past its 30 second budget and was cancelled.
    QueryTimeout = "query_timeout", 503;
    /// The server is in setup mode: only the setup wizard answers until an admin exists
    /// (decision 0021).
    SetupRequired = "setup_required", 503;
    /// The setup token is missing or wrong. Shares the sign-in limit of 10 a minute.
    SetupTokenInvalid = "setup_token_invalid", 401;
    /// The setup wizard's import is running, or not finished yet, so it cannot start again, be
    /// discarded or be acknowledged.
    ImportInProgress = "import_in_progress", 409;
    Internal = "internal", 500;
}

/// Why one field failed validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FieldErrorCode {
    Required,
    TooShort,
    TooLong,
    /// More items than the limit allows.
    TooMany,
    InvalidFormat,
    InvalidValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct FieldError {
    pub code: FieldErrorCode,
    /// The limit that was exceeded, for `too_short`, `too_long` and `too_many`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub limit: Option<u32>,
}

/// The body of every non-2xx API answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct ApiErrorBody {
    pub error_code: ErrorCode,
    /// English text for logs and developers. Not shown to people and not stable.
    pub message: String,
    /// Field errors keyed by argument name, for `invalid_arguments`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub errors: Option<BTreeMap<String, Vec<FieldError>>>,
    /// The current revision, for `stale_revision`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub current: Option<i64>,
    /// What blocks the erasure, for `user_owns_resources`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub owned: Option<Vec<OwnedResource>>,
}

/// A workspace or site that must be transferred or deleted before its owner can be erased.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnedResource {
    Workspace { id: WorkspaceId, name: String },
    Site { id: SiteId, name: String },
}

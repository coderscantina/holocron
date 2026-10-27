//! Scoped API keys of the caller. A key carries operation and site grants and acts as its owner
//! with the owner's current capabilities, never more (R-KEY-CEILING). Keys never manage keys.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::envelope::Empty;
use crate::ids::{ApiKeyId, SiteId};

crate::domain! {
    pub trait ApiKeysHandlers;
    commands {
        /// Creates a key and discloses its secret once. Needs reauthentication. Every operation
        /// must be key-grantable in the registry (`keyGrantableOperations` in the generated
        /// code) and within the caller's current capabilities on every listed site, else
        /// `grant_exceeds_principal`. A site the caller cannot see is `not_found`. Unknown
        /// operation names are `invalid_arguments`. At most 50 active keys per person.
        "apiKeys.create" => api_keys_create (Person, reauth, secret): ApiKeysCreateArgs => ApiKeyCreated;
        /// Revokes one of the caller's keys at once. Requests with it answer like an unknown
        /// key, and its subscriptions end. Revoking a revoked key answers the key unchanged.
        "apiKeys.revoke" => api_keys_revoke (Person): ApiKeyArgs => ApiKeyResult;
    }
    queries {
        /// The caller's keys, newest first, revoked ones included. Never the secret.
        "apiKeys.list" => api_keys_list (Person): Empty => ApiKeyList;
    }
}

/// The "Read analytics" grant preset for agent keys (decision 0012): reports, exports, field
/// lookups, freshness and site metadata. No session timelines, realtime or recordings.
pub const READ_ANALYTICS_PRESET: &[&str] = &[
    "analytics.report",
    "analytics.export",
    "fields.catalog",
    "fields.values",
    "analytics.freshness",
    "sites.get",
    "sites.list",
];

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ApiKey {
    pub id: ApiKeyId,
    /// 1 to 100 characters after trimming.
    pub name: String,
    /// The first characters of the secret, to tell keys apart.
    pub prefix: String,
    /// Operation names the key may call.
    pub operations: Vec<String>,
    pub sites: KeySites,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_used_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub revoked_at: Option<i64>,
}

/// Which sites a key reaches, on top of what its owner can currently reach.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum KeySites {
    /// Every site the owner can read, now and later.
    All,
    /// Only these sites. Other sites answer `not_found`.
    Only { sites: Vec<SiteId> },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ApiKeysCreateArgs {
    pub name: String,
    /// 1 or more operation names.
    pub operations: Vec<String>,
    pub sites: KeySites,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ApiKeyCreated {
    pub key: ApiKey,
    /// The full secret for `Authorization: Bearer <secret>`. `null` in a replayed answer.
    pub secret: Option<String>,
    /// True when this answer is a replay and the secret was withheld.
    pub secret_withheld: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ApiKeyArgs {
    pub key: ApiKeyId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ApiKeyResult {
    pub key: ApiKey,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ApiKeyList {
    pub keys: Vec<ApiKey>,
}

#[cfg(test)]
mod tests {
    use super::READ_ANALYTICS_PRESET;
    use crate::operations;
    use crate::registry::Kind;

    #[test]
    fn every_preset_operation_is_a_key_grantable_query() {
        for name in READ_ANALYTICS_PRESET {
            let op = operations::find(Kind::Query, name).unwrap_or_else(|| panic!("{name}"));
            assert!(op.policy.key_grantable, "{name} is not key-grantable");
        }
    }
}

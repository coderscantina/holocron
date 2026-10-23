//! Putting collection on a site: the tracking snippet (CAP-COLLECT-01) and the site collection
//! keys of trusted server integrations (decision 0001, Transient inputs and Persistent consent
//! transport).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ids::{CollectionKeyId, SiteId};
use crate::operations::sites::SitesGetArgs;

crate::domain! {
    pub trait TrackingHandlers;
    commands {
        /// Creates a collection key for a site and discloses its secret once. Needs
        /// `site.configure`. A `POST /api/send` or `/api/batch` carrying it as
        /// `Authorization: Bearer <secret>` is trusted for this site: `payload.ip` and
        /// `payload.userAgent` replace the connection's. At most 20 active keys per site.
        "collectionKeys.create" => collection_keys_create (Person, secret): CollectionKeysCreateArgs => CollectionKeyCreated;
        /// Revokes one of the site's collection keys at once; requests carrying it are refused.
        /// Needs `site.configure`. Revoking a revoked key answers the key unchanged.
        "collectionKeys.revoke" => collection_keys_revoke (Person): CollectionKeyArgs => CollectionKeyResult;
    }
    queries {
        /// The `<script>` tags that put the pinned tracker, and the recorder when the site
        /// records, on a page. Needs `site.configure`.
        "sites.trackingCode" => sites_tracking_code (Person): SitesTrackingCodeArgs => TrackingCode;
        /// The site's collection keys, newest first, revoked ones included. Never the secret.
        /// Needs `site.configure`.
        "collectionKeys.list" => collection_keys_list (Person): SitesGetArgs => CollectionKeyList;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SitesTrackingCodeArgs {
    pub site: SiteId,
    /// Origin the browser reached the dashboard at, such as `https://stats.example.com`. Used
    /// only when the server has no public URL configured, as upstream falls back to the page's
    /// origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub origin: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct TrackingCode {
    /// The tracker under every name it is served at: configured aliases first, then
    /// `script.js`. The first is the one to use, as upstream shows its first alias.
    pub tracker: Vec<ScriptTag>,
    /// The recorder, present while replays or heatmaps are enabled for the site.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub recorder: Option<ScriptTag>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ScriptTag {
    /// Path the script is served at, for example `/script.js`.
    pub path: String,
    /// The `<script>` tag to paste into the page's `<head>`.
    pub code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct CollectionKey {
    pub id: CollectionKeyId,
    pub site: SiteId,
    /// 1 to 100 characters after trimming.
    pub name: String,
    /// The first characters of the secret, to tell keys apart.
    pub prefix: String,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub revoked_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct CollectionKeysCreateArgs {
    pub site: SiteId,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct CollectionKeyCreated {
    pub key: CollectionKey,
    /// The full secret for `Authorization: Bearer <secret>`. `null` in a replayed answer.
    pub secret: Option<String>,
    /// True when this answer is a replay and the secret was withheld.
    pub secret_withheld: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct CollectionKeyArgs {
    pub site: SiteId,
    pub key: CollectionKeyId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct CollectionKeyResult {
    pub key: CollectionKey,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct CollectionKeyList {
    pub keys: Vec<CollectionKey>,
}

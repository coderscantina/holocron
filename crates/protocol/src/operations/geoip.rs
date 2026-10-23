//! The GeoIP database that locates visitors no trusted CDN header located (decision 0018): the
//! operator's file from `HOLOCRON_GEOIP_DB`, or DB-IP Lite City downloaded by an install admin.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::envelope::Empty;

crate::domain! {
    pub trait GeoipHandlers;
    commands {
        /// Starts downloading the newest DB-IP Lite City, which replaces the installed one once
        /// it is verified. Answers at once; `geoip.settings` tells when it is done. Starts
        /// nothing while a download runs or while `HOLOCRON_GEOIP_DB` names the database.
        /// Install admins only.
        "geoip.download" => geoip_download (Person): Empty => GeoipSettings;
    }
    queries {
        /// The database lookups use now, where it comes from and the download state. Install
        /// admins only.
        "geoip.settings" => geoip_settings (Person): Empty => GeoipSettings;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct GeoipSettings {
    /// The database lookups use now. Absent without one, or when the operator's file could
    /// not be opened.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub database: Option<GeoipDatabase>,
    /// `HOLOCRON_GEOIP_DB` names the database, so downloads are off.
    pub external: bool,
    /// The newest month `YYYY-MM` DB-IP has published. Only looked up while the installed
    /// month is older than the current one; absent otherwise or when DB-IP did not answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub latest_month: Option<String>,
    pub download: GeoipDownload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct GeoipDatabase {
    /// The file's own type, such as `DBIP-City-Lite` or `GeoLite2-City`.
    pub database_type: String,
    /// When the file was built, Unix milliseconds.
    pub built_at: i64,
    /// The month `YYYY-MM` of a downloaded DB-IP file. Absent for the operator's file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub month: Option<String>,
    /// DB-IP's data, whose licence needs a link to db-ip.com where its results show.
    pub dbip: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum GeoipDownload {
    Idle,
    Running,
    /// The last download failed; the installed database, if any, keeps serving.
    Failed {
        reason: GeoipFailure,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GeoipFailure {
    /// DB-IP could not be reached, or the transfer broke off.
    Unreachable,
    /// Neither this month's nor last month's file is published.
    NotPublished,
    /// The file is not a MaxMind DB City database, or larger than 512 MiB unpacked.
    Invalid,
    /// Free space is below the disk reserve plus 512 MiB, or the file could not be written.
    DiskFull,
}

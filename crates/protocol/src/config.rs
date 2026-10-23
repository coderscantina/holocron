//! Deployment settings the web app reads at boot from `/config.json`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Served uncached, so a changed server setting reaches the browser without rebuilding the app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct RuntimeConfig {
    /// Path prefix of the dashboard API, without a trailing slash.
    pub api_base: String,
    /// Server version, shown in the sidebar footer.
    pub version: String,
    /// The operator's name for this installation, shown in the shell and the document title.
    pub instance_name: String,
    /// The GeoIP database in use is DB-IP's, whose licence needs a link to db-ip.com on pages
    /// that show location (decision 0018).
    pub dbip_attribution: bool,
    /// The server runs in setup mode: only `/setup` and its endpoints answer (decision 0021).
    pub setup: bool,
}

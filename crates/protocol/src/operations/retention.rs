//! The installation's default event retention (decision 0016). Sites follow it unless they set
//! their own with `sites.updateRetention`; links and pixels always follow it, except where an
//! import set theirs.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::envelope::Empty;
use crate::operations::sites::RetentionPreview;
use crate::retention::Retention;

crate::domain! {
    pub trait RetentionHandlers;
    commands {
        /// Changes the installation default. Needs `install.admin`. Shortening needs
        /// reauthentication and applies to every site, link and pixel without its own setting.
        /// An unchanged setting records nothing.
        "retention.updateDefault" => retention_update_default (Person, reauth_by_handler): RetentionDefaultArgs => RetentionSettings;
    }
    queries {
        /// The installation default and whether the caller may change it.
        "retention.settings" => retention_settings (Person): Empty => RetentionSettings;
        /// What `retention.updateDefault` with the same arguments would remove, summed over
        /// the sources that follow the default. Needs `install.admin`.
        "retention.previewDefault" => retention_preview_default (Person): RetentionDefaultArgs => RetentionPreview;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct RetentionSettings {
    pub installation_default: Retention,
    pub can_manage_installation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct RetentionDefaultArgs {
    pub retention: Retention,
}

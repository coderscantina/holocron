//! Links (`/q/<slug>`) and pixels (`/p/<slug>`) of a workspace (decision 0020). A role has
//! the same capabilities on them as on a site of the workspace: `site.read` lists them and reads
//! their reports, `site.configure` changes them, `site.delete` deletes them. Creating one needs
//! `workspace.write`, like creating a site. API keys reach them only when they hold every site.
//!
//! A slug is 1 to 100 letters, digits, `-` or `_`, unique among links and, apart, among
//! pixels. Without one the server picks 8 random characters. A changed slug answers at once and
//! the old one stops answering. Deleting erases the source's hits.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::access::Capability;
use crate::exploration::ExploreResult;
use crate::ids::{LinkId, PixelId, WorkspaceId};
use crate::operations::explorations::{Deleted, ExploreArgs};

crate::domain! {
    pub trait LinksHandlers;
    commands {
        /// Creates a link in a workspace. Needs `workspace.write`. The URL must be absolute
        /// `http` or `https` without credentials.
        "links.create" => links_create (Person, key): LinksCreateArgs => LinkResult;
        /// Renames a link, changes its URL or its slug. Needs `site.configure`.
        "links.update" => links_update (Person, key): LinksUpdateArgs => LinkResult;
        /// Deletes a link and erases its hits. Needs `site.delete` and reauthentication.
        "links.delete" => links_delete (Person, reauth): LinkArgs => Deleted;
        /// Creates a pixel in a workspace. Needs `workspace.write`.
        "pixels.create" => pixels_create (Person, key): PixelsCreateArgs => PixelResult;
        /// Renames a pixel or changes its slug. Needs `site.configure`.
        "pixels.update" => pixels_update (Person, key): PixelsUpdateArgs => PixelResult;
        /// Deletes a pixel and erases its hits. Needs `site.delete` and reauthentication.
        "pixels.delete" => pixels_delete (Person, reauth): PixelArgs => Deleted;
    }
    queries {
        /// The workspace's links by name. Needs `site.read`.
        "links.list" => links_list (Person, key): SourcesListArgs => LinkList;
        /// An exploration of a link's hits. The state's `site` is the link. Hits count as
        /// `events`; the measures are `events`, `visitors` and `visitor_days`, without a report.
        /// Visitors are counted by UTC day. Needs `site.read`.
        "links.explore" => links_explore (Person, key): ExploreArgs => ExploreResult;
        /// The workspace's pixels by name. Needs `site.read`.
        "pixels.list" => pixels_list (Person, key): SourcesListArgs => PixelList;
        /// `links.explore` for a pixel.
        "pixels.explore" => pixels_explore (Person, key): ExploreArgs => ExploreResult;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct Link {
    pub id: LinkId,
    pub workspace: WorkspaceId,
    pub name: String,
    pub slug: String,
    /// Where `/q/<slug>` redirects.
    pub url: String,
    /// Hits since creation that could not be stored during a storage outage (0005).
    pub lost_hits: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct Pixel {
    pub id: PixelId,
    pub workspace: WorkspaceId,
    pub name: String,
    pub slug: String,
    /// Hits since creation that could not be stored during a storage outage (0005).
    pub lost_hits: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct LinkList {
    pub links: Vec<Link>,
    /// What the caller may do with the workspace's links and pixels.
    pub capabilities: Vec<Capability>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PixelList {
    pub pixels: Vec<Pixel>,
    /// What the caller may do with the workspace's links and pixels.
    pub capabilities: Vec<Capability>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct LinkResult {
    pub link: Link,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PixelResult {
    pub pixel: Pixel,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SourcesListArgs {
    pub workspace: WorkspaceId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct LinkArgs {
    pub link: LinkId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PixelArgs {
    pub pixel: PixelId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct LinksCreateArgs {
    pub workspace: WorkspaceId,
    /// 1 to 100 characters after trimming.
    pub name: String,
    pub url: String,
    /// Picked by the server when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub slug: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct LinksUpdateArgs {
    pub link: LinkId,
    pub name: String,
    pub url: String,
    pub slug: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PixelsCreateArgs {
    pub workspace: WorkspaceId,
    /// 1 to 100 characters after trimming.
    pub name: String,
    /// Picked by the server when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub slug: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PixelsUpdateArgs {
    pub pixel: PixelId,
    pub name: String,
    pub slug: String,
}

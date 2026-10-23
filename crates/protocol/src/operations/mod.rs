//! Every dashboard operation, grouped by domain. A new domain adds a module and one line below.

pub mod account;
pub mod analytics;
pub mod api_keys;
pub mod auth;
pub mod boards;
pub mod collection;
pub mod explorations;
pub mod geoip;
pub mod goals;
pub mod links;
pub mod operator;
pub mod recordings;
pub mod retention;
pub mod security;
pub mod sharing;
pub mod site_management;
pub mod sites;
pub mod summaries;
pub mod teams;
pub mod tracking;
pub mod two_factor;
pub mod users;
pub mod workspaces;

crate::registry::domains!(
    account::AccountHandlers,
    analytics::AnalyticsHandlers,
    api_keys::ApiKeysHandlers,
    auth::AuthHandlers,
    boards::BoardsHandlers,
    collection::CollectionHandlers,
    recordings::RecordingsHandlers,
    retention::RetentionHandlers,
    explorations::ExplorationsHandlers,
    geoip::GeoipHandlers,
    goals::GoalsHandlers,
    links::LinksHandlers,
    operator::OperatorHandlers,
    security::SecurityHandlers,
    sharing::SharingHandlers,
    site_management::SiteManagementHandlers,
    sites::SitesHandlers,
    summaries::SummariesHandlers,
    teams::TeamsHandlers,
    tracking::TrackingHandlers,
    two_factor::TwoFactorHandlers,
    users::UsersHandlers,
    workspaces::WorkspacesHandlers,
);

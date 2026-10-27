//! Holocron server: public collection, dashboard API, runtime configuration and the built web
//! app in one process.

pub mod background;
pub mod clock;
pub mod collect;
pub mod config;
pub mod consents;
pub mod context;
pub mod db;
pub mod error;
pub mod events;
pub mod explorations;
pub mod http;
pub mod idempotency;
pub mod identity;
pub mod imports;
pub mod init;
pub mod links;
pub mod maintenance;
pub mod pseudonyms;
pub mod query_cli;
pub mod recordings;
pub mod retention;
pub mod setup;
pub mod sharing;
pub mod sites;
pub mod state;
pub mod summaries;
pub mod teams;

pub use http::router;
pub use state::AppState;

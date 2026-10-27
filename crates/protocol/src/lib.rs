//! Holocron dashboard wire protocol.
//!
//! The operation registry in [`operations`] is the single source for dispatch, the generated
//! TypeScript in `apps/web/src/generated` and the OpenAPI document. Run
//! `cargo run -p holocron-protocol --bin export-types` after changing a wire type, and
//! `-- --check` to verify the committed output has not drifted.
//!
//! Wire conventions are fixed once: snake_case fields, timestamps as Unix milliseconds, IDs as
//! strings, and absent optional fields left out rather than sent as `null`.

pub mod access;
pub mod config;
pub mod country_centroids;
pub mod envelope;
pub mod errors;
pub mod exploration;
pub mod export;
pub mod ids;
pub mod limits;
pub mod operations;
pub mod registry;
pub mod report_query;
pub mod retention;
pub mod setup;

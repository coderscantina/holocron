//! Derived analytics on top of the analytical store: identity days and policy
//! versions, inactivity sessions, bounded idempotent correction driven by dirty
//! identity days, versioned day aggregates, and the freshness every result carries.
//!
//! Immutable events, identify messages, replay fragments and heatmap
//! interactions stay as stored. Sessions, their members and aggregates are
//! derived from them and can be rebuilt at any time.

pub mod catalog;
pub mod correction;
pub mod explore;
pub mod freshness;
pub mod identity;
pub mod jobs;
pub mod realtime;
pub mod report_query;
pub mod traffic;

pub use holocron_protocol::country_centroids;

pub use correction::{Limits, Step, follow, run_until_clean, step};
pub use freshness::{Correction, Freshness, ImportedHistory, freshness};

/// Formula version of every result (0003). Any change to a formula bumps it. The protocol
/// carries it so the browser pins new states to it.
pub use holocron_protocol::limits::METRIC_VERSION;

//! The field catalog of a site: custom property keys with the value types stored under them.

use holocron_protocol::exploration::PropertyScope;
use holocron_storage::duckdb::{self, params};
use holocron_storage::{Id, Snapshot};

/// Most keys one catalog lists.
pub const KEYS_MAX: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    pub scope: PropertyScope,
    pub key: String,
    /// Stored type tags, ascending (`holocron_storage::property_type`).
    pub types: Vec<u8>,
    pub rows: i64,
}

/// Event and session property keys of a site, most used first, then by key.
pub fn property_keys(snapshot: &Snapshot, site: Id) -> duckdb::Result<Vec<Key>> {
    let mut statement = snapshot.connection().prepare(&format!(
        "SELECT scope, key, list(DISTINCT type ORDER BY type), count(*) AS rows FROM (
             SELECT 1 AS scope, key, type FROM event_properties WHERE site = ?
             UNION ALL SELECT 2, key, type FROM session_properties WHERE site = ?)
         GROUP BY scope, key ORDER BY rows DESC, key, scope LIMIT {KEYS_MAX}"
    ))?;
    statement
        .query_map(params![site, site], |r| {
            let types = match r.get::<_, duckdb::types::Value>(2)? {
                duckdb::types::Value::List(items) => items
                    .into_iter()
                    .filter_map(|v| match v {
                        duckdb::types::Value::UTinyInt(t) => Some(t),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            };
            Ok(Key {
                scope: if r.get::<_, i32>(0)? == 1 {
                    PropertyScope::Event
                } else {
                    PropertyScope::Session
                },
                key: r.get(1)?,
                types,
                rows: r.get(3)?,
            })
        })?
        .collect()
}

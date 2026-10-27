//! Two resources: the report query JSON Schema, generated from the protocol types, and the guide
//! with worked examples, checked in next to this file.

use holocron_protocol::report_query::ReportQuery;
use serde::Serialize;
use serde_json::{Map, Value, json};

use super::tools::schema;
use super::{Era, INVALID_PARAMS, RpcError};

pub(super) const SCHEMA_URI: &str = "holocron://report-query/schema";
pub(super) const GUIDE_URI: &str = "holocron://report-query/guide";

/// How report queries work, with worked examples. Also linked from the user guide.
const GUIDE: &str = include_str!("guide.md");

/// Resource-not-found before 2026-07-28.
const LEGACY_NOT_FOUND: i64 = -32002;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Resource {
    uri: &'static str,
    name: &'static str,
    title: &'static str,
    description: &'static str,
    mime_type: &'static str,
}

const RESOURCES: [Resource; 2] = [
    Resource {
        uri: SCHEMA_URI,
        name: "report-query-schema",
        title: "Report query schema",
        description: "JSON Schema of the `query` tool's input, a ReportQuery.",
        mime_type: "application/schema+json",
    },
    Resource {
        uri: GUIDE_URI,
        name: "report-query-guide",
        title: "Report query guide",
        description: "How blocks, for_each, refs, derived columns, funnels, globs, limits and \
            notes work, with worked examples. Read it before writing a query.",
        mime_type: "text/markdown",
    },
];

pub(super) fn list() -> Value {
    json!({ "resources": RESOURCES })
}

pub(super) fn read(params: &Map<String, Value>, era: Era) -> Result<Value, RpcError> {
    let uri = params
        .get("uri")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let text = match uri {
        SCHEMA_URI => serde_json::to_string_pretty(&schema::<ReportQuery>())
            .map_err(|error| RpcError::new(-32603, error.to_string()))?,
        GUIDE_URI => GUIDE.to_owned(),
        _ => {
            let code = if era.modern() {
                INVALID_PARAMS
            } else {
                LEGACY_NOT_FOUND
            };
            return Err(RpcError {
                code,
                message: "Resource not found".into(),
                data: Some(json!({ "uri": uri })),
            });
        }
    };
    let mime_type = RESOURCES
        .iter()
        .find(|resource| resource.uri == uri)
        .map(|resource| resource.mime_type);
    Ok(json!({ "contents": [{ "uri": uri, "mimeType": mime_type, "text": text }] }))
}

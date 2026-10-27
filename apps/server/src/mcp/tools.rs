//! The five tools. Each runs one query operation as the calling key, and answers its result as
//! JSON text, or a refusal with its error code, message and field paths as a tool error the
//! agent can correct.

use holocron_protocol::envelope::Empty;
use holocron_protocol::errors::ApiErrorBody;
use holocron_protocol::errors::ErrorCode;
use holocron_protocol::exploration::{ExplorationState, Field};
use holocron_protocol::ids::SiteId;
use holocron_protocol::operations;
use holocron_protocol::operations::explorations::{
    ExportArgs, ExportFormat, FieldsCatalogArgs, FieldsValuesArgs,
};
use holocron_protocol::registry::{ArgsSource, JsonArgs, Kind};
use holocron_protocol::report_query::ReportQuery;
use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::{INVALID_PARAMS, RpcError, Text};
use crate::error::ApiError;
use crate::http::operations::read;
use crate::identity::Caller;
use crate::state::AppState;

struct Tool {
    name: &'static str,
    title: &'static str,
    operation: &'static str,
    description: &'static str,
    input_schema: fn() -> Value,
    /// The operation's arguments from the tool's.
    args: fn(Value) -> Result<Value, ApiError>,
}

const TOOLS: [Tool; 5] = [
    Tool {
        name: "list_sites",
        title: "List sites",
        operation: "sites.list",
        description: "Lists the sites this key can read: id, name, domain and workspace. Call it \
            first: every other tool takes a site id from here.",
        input_schema: schema::<Empty>,
        args: Ok,
    },
    Tool {
        name: "catalog",
        title: "Property catalog",
        operation: "fields.catalog",
        description: "Lists the custom event and session property keys seen on one site, with \
            their value types. Use it before you filter, group or sum by a property, so a field \
            such as {\"scope\":\"event\",\"key\":\"checkout_id\"} names a key that exists.",
        input_schema: schema::<FieldsCatalogArgs>,
        args: Ok,
    },
    Tool {
        name: "field_values",
        title: "Field values",
        operation: "fields.values",
        description: "Returns the most frequent values of one field on one site over the last 30 \
            days, optionally only those starting with `prefix`. Use it to find exact event names, \
            paths, campaigns or property values before you filter on them. `field` is written as \
            in a report query: \"event_name\", \"path\" or {\"scope\":\"event\",\"key\":\"plan\"}.",
        input_schema: schema::<FieldValuesInput>,
        args: field_values_args,
    },
    Tool {
        name: "query",
        title: "Report query",
        operation: "analytics.report",
        description: "Runs a report query: up to 16 named blocks, each a cut (measures by up to \
            three dimensions) or a report (funnel, journey, attribution, retention, goals) on one \
            site, all from one snapshot within 30 seconds. Answers one typed table per block, with \
            notes to cite as method notes. Prefer one query with several blocks over many calls. \
            Read the resource holocron://report-query/guide for the rules and worked examples. A \
            refusal names the path to fix, such as blocks.2.filter.f3.",
        input_schema: schema::<ReportQuery>,
        args: Ok,
    },
    Tool {
        name: "export",
        title: "Export rows",
        operation: "analytics.export",
        description: "Exports every row, up to 10,000, of one exploration state as CSV or JSON \
            text, with the state and versions it came from. Use it when a cut needs more than the \
            1,000 rows a query block returns, for example every order by event.checkout_id to \
            join with external data. Needs the export permission on the site.",
        input_schema: schema::<ExportInput>,
        args: export_args,
    },
];

/// `fields.values` takes the field as JSON text; agents send it as in a report query.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct FieldValuesInput {
    site: SiteId,
    field: Field,
    /// Only values starting with this text, compared case-insensitively.
    #[serde(default)]
    prefix: Option<String>,
}

/// `analytics.export` takes the state as JSON text; agents send it as an object.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ExportInput {
    /// The exploration to export. Every row is exported, whatever its page.
    state: ExplorationState,
    format: ExportFormat,
}

fn field_values_args(arguments: Value) -> Result<Value, ApiError> {
    let input: FieldValuesInput = JsonArgs(arguments).parse().map_err(malformed)?;
    let args = FieldsValuesArgs {
        site: input.site,
        field: serde_json::to_string(&input.field).map_err(ApiError::internal)?,
        prefix: input.prefix,
    };
    serde_json::to_value(args).map_err(ApiError::internal)
}

fn export_args(arguments: Value) -> Result<Value, ApiError> {
    let input: ExportInput = JsonArgs(arguments).parse().map_err(malformed)?;
    let args = ExportArgs {
        state: serde_json::to_string(&input.state).map_err(ApiError::internal)?,
        format: input.format,
    };
    serde_json::to_value(args).map_err(ApiError::internal)
}

fn malformed(message: String) -> ApiError {
    ApiError::with_message(ErrorCode::MalformedRequest, message)
}

/// The root JSON Schema of `T`, inlined for a tool's input or a resource.
pub(super) fn schema<T: JsonSchema>() -> Value {
    let mut settings = SchemaSettings::draft2020_12();
    settings.meta_schema = None;
    settings
        .into_generator()
        .into_root_schema_for::<T>()
        .to_value()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolInfo {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    input_schema: Value,
    annotations: Value,
}

pub(super) fn list() -> Value {
    let tools: Vec<ToolInfo> = TOOLS
        .iter()
        .map(|tool| ToolInfo {
            name: tool.name,
            title: tool.title,
            description: tool.description,
            input_schema: (tool.input_schema)(),
            annotations: json!({ "readOnlyHint": true, "openWorldHint": false }),
        })
        .collect();
    json!({ "tools": tools })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolResult {
    content: [Text; 1],
    #[serde(skip_serializing_if = "Option::is_none")]
    structured_content: Option<Value>,
    is_error: bool,
}

/// `tools/call`. An unknown tool is a protocol error; anything the operation refuses is a
/// tool error, so the agent sees it and can fix its arguments.
pub(super) async fn call(
    state: &AppState,
    caller: Caller,
    params: &Map<String, Value>,
    structured: bool,
) -> Result<Value, RpcError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let tool = TOOLS
        .iter()
        .find(|tool| tool.name == name)
        .ok_or_else(|| RpcError::new(INVALID_PARAMS, format!("Unknown tool: {name}")))?;
    let arguments = match params.get("arguments") {
        None => Value::Object(Map::new()),
        Some(arguments @ Value::Object(_)) => arguments.clone(),
        Some(_) => {
            return Err(RpcError::new(
                INVALID_PARAMS,
                "arguments must be an object.",
            ));
        }
    };
    let (text, value, is_error) = match run(state, caller, tool, arguments).await {
        Ok(answer) => {
            let answer = Value::Object(answer);
            (answer.to_string(), answer, false)
        }
        Err(error) => {
            let body = error.body();
            let value = serde_json::to_value(body).unwrap_or_default();
            (refusal(body), value, true)
        }
    };
    let result = ToolResult {
        content: [Text::new(text)],
        structured_content: structured.then_some(value),
        is_error,
    };
    serde_json::to_value(result).map_err(|error| RpcError::new(-32603, error.to_string()))
}

/// Admission, then the operation's arguments, then one read within the request budget, as on
/// `POST /api/queries/{name}`. The answer drops the HTTP envelope's revision and cursor.
async fn run(
    state: &AppState,
    caller: Caller,
    tool: &Tool,
    arguments: Value,
) -> Result<Map<String, Value>, ApiError> {
    let op = operations::find(Kind::Query, tool.operation)
        .ok_or_else(|| ApiError::internal(format!("{} is not a query", tool.operation)))?;
    caller.admit(op)?;
    let args = (tool.args)(arguments)?;
    let mut answer = read(state, op, caller, &JsonArgs(args)).await?;
    answer.remove("revision");
    answer.remove("cursor");
    Ok(answer)
}

/// `error_code: message`, then one line per refused field path with its codes.
fn refusal(body: &ApiErrorBody) -> String {
    let mut text = format!("{}: {}", body.error_code.as_str(), body.message);
    for (path, errors) in body.errors.iter().flatten() {
        let codes: Vec<String> = errors
            .iter()
            .map(|error| {
                let code = serde_json::to_value(error.code)
                    .ok()
                    .and_then(|code| code.as_str().map(str::to_owned))
                    .unwrap_or_default();
                match error.limit {
                    Some(limit) => format!("{code} (limit {limit})"),
                    None => code,
                }
            })
            .collect();
        text.push_str(&format!("\n{path}: {}", codes.join(", ")));
    }
    text
}

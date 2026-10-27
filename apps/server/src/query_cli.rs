//! `holocron query`: posts a JSON query to a running server and returns its answer. It sends
//! the file as it is, so it knows no operation's types, and it never opens the data directory.

use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

use clap::Args;
use holocron_protocol::limits::QUERY_BODY_MAX_BYTES;
use serde_json::Value;

use crate::init::{Exit, Failure};

/// The environment variable holding the API key. It is never a flag, so it stays out of shell
/// history and process lists.
pub const KEY_ENV: &str = "HOLOCRON_API_KEY";

/// Answers are not capped by the server, but a report is far below this.
const ANSWER_MAX_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Args)]
pub struct QueryArgs {
    /// The server's origin, such as https://stats.example.com.
    #[arg(long, env = "HOLOCRON_URL")]
    url: String,
    /// The operation to call.
    #[arg(long, default_value = "analytics.report")]
    operation: String,
    /// File holding the query as a JSON object, or `-` for stdin.
    input: PathBuf,
}

/// Sends the query and returns the answer. Blocking: call it from a blocking context.
pub fn run(args: &QueryArgs, key: Option<String>) -> Result<Value, Failure> {
    let usage = |message: String| Failure::new(Exit::Usage, "usage", message);
    let key = key
        .filter(|key| !key.is_empty())
        .ok_or_else(|| usage(format!("set {KEY_ENV} to an API key")))?;
    let body = read_input(&args.input).map_err(usage)?;

    let url = format!(
        "{}/api/queries/{}",
        args.url.trim_end_matches('/'),
        args.operation
    );
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(Some(Duration::from_secs(60)))
            .user_agent(concat!("holocron/", env!("CARGO_PKG_VERSION")))
            .build(),
    );
    let internal =
        |code: &'static str, message: String| Failure::new(Exit::Internal, code, message);
    let mut response = agent
        .post(&url)
        .header("Authorization", format!("Bearer {key}"))
        .content_type("application/json")
        .send(&body)
        .map_err(|e| internal("unreachable", format!("{url}: {e}")))?;
    let status = response.status();
    let bytes = response
        .body_mut()
        .with_config()
        .limit(ANSWER_MAX_BYTES)
        .read_to_vec()
        .map_err(|e| internal("unreachable", format!("{url}: {e}")))?;
    let answer = serde_json::from_slice::<Value>(&bytes);

    if status.is_success() {
        return answer.map_err(|e| internal("bad_answer", format!("{url}: not JSON: {e}")));
    }
    Err(match answer {
        Ok(envelope) if envelope["error_code"].is_string() => refusal(&envelope),
        _ => internal("http_error", format!("{url}: status {status}")),
    })
}

/// The input must be one JSON object under the server's body limit.
fn read_input(path: &PathBuf) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let limit = QUERY_BODY_MAX_BYTES as u64 + 1;
    let read = if path.as_os_str() == "-" {
        std::io::stdin().take(limit).read_to_end(&mut bytes)
    } else {
        std::fs::File::open(path).and_then(|file| file.take(limit).read_to_end(&mut bytes))
    };
    read.map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() > QUERY_BODY_MAX_BYTES {
        return Err(format!(
            "the query is over the server's limit of {QUERY_BODY_MAX_BYTES} bytes"
        ));
    }
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(Value::Object(_)) => Ok(bytes),
        Ok(_) => Err("the query must be a JSON object".into()),
        Err(e) => Err(format!("the query is not valid JSON: {e}")),
    }
}

/// An error envelope as a failure: `error_code` as the code, the message and one line per
/// field path.
fn refusal(envelope: &Value) -> Failure {
    let mut message = envelope["message"].as_str().unwrap_or_default().to_owned();
    if let Some(fields) = envelope["errors"].as_object() {
        for (path, errors) in fields {
            let codes: Vec<String> = errors
                .as_array()
                .into_iter()
                .flatten()
                .map(|error| match error["limit"].as_u64() {
                    Some(limit) => {
                        format!("{} (limit {limit})", error["code"].as_str().unwrap_or("?"))
                    }
                    None => error["code"].as_str().unwrap_or("?").to_owned(),
                })
                .collect();
            message.push_str(&format!("\n  {path}: {}", codes.join(", ")));
        }
    }
    Failure::new(
        Exit::Refused,
        envelope["error_code"]
            .as_str()
            .unwrap_or("error")
            .to_owned(),
        message,
    )
}

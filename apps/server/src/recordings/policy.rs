//! The recording policy of a site (CAP-RECORD-01): off until enabled, strict masking always,
//! 10% sampling by default, element and page exclusions. Control storage owns it.

use holocron_protocol::errors::FieldErrorCode;
use holocron_protocol::operations::recordings::{RecordingPolicy, RecordingPolicyUpdateArgs};
use serde_json::{Value, json};
use sqlx::{Executor, Sqlite, SqliteConnection};

use crate::error::{ApiError, FieldErrors};

pub const DEFAULT_SAMPLE_PERCENT: u8 = 10;
pub const DEFAULT_MAX_DURATION_MINUTES: u16 = 5;
const MAX_ELEMENTS: usize = 20;
const MAX_PATHS: usize = 50;
const MAX_ENTRY_CHARS: usize = 200;

/// The policy of a site without a stored one: nothing is recorded.
pub fn disabled() -> RecordingPolicy {
    RecordingPolicy {
        replay_enabled: false,
        heatmap_enabled: false,
        replay_sample_percent: DEFAULT_SAMPLE_PERCENT,
        heatmap_sample_percent: DEFAULT_SAMPLE_PERCENT,
        max_duration_minutes: DEFAULT_MAX_DURATION_MINUTES,
        excluded_elements: Vec::new(),
        excluded_paths: Vec::new(),
        version: 0,
    }
}

type PolicyRow = (bool, bool, i64, i64, i64, String, String, i32);

/// The stored policy of `site`, or the disabled default.
pub async fn load<'e>(
    conn: impl Executor<'e, Database = Sqlite>,
    site: &str,
) -> Result<RecordingPolicy, sqlx::Error> {
    let row: Option<PolicyRow> = sqlx::query_as(
        "SELECT replay_enabled, heatmap_enabled, replay_sample_percent, heatmap_sample_percent,
                max_duration_minutes, excluded_elements, excluded_paths, version
         FROM recording_policies WHERE site_id = ?",
    )
    .bind(site)
    .fetch_optional(conn)
    .await?;
    let Some((replay, heatmap, replay_rate, heatmap_rate, duration, elements, paths, version)) =
        row
    else {
        return Ok(disabled());
    };
    let list = |text: &str| {
        serde_json::from_str::<Vec<String>>(text).map_err(|e| sqlx::Error::Decode(e.into()))
    };
    Ok(RecordingPolicy {
        replay_enabled: replay,
        heatmap_enabled: heatmap,
        replay_sample_percent: replay_rate as u8,
        heatmap_sample_percent: heatmap_rate as u8,
        max_duration_minutes: duration as u16,
        excluded_elements: list(&elements)?,
        excluded_paths: list(&paths)?,
        version,
    })
}

/// The policy `args` ask for, trimmed and validated. `version` is left at the current one.
pub fn validate(
    args: &RecordingPolicyUpdateArgs,
    version: i32,
) -> Result<RecordingPolicy, ApiError> {
    let mut errors = FieldErrors::default();
    if !(1..=100).contains(&args.replay_sample_percent) {
        errors.add("replay_sample_percent", FieldErrorCode::InvalidValue, None);
    }
    if !(1..=100).contains(&args.heatmap_sample_percent) {
        errors.add("heatmap_sample_percent", FieldErrorCode::InvalidValue, None);
    }
    if !(1..=60).contains(&args.max_duration_minutes) {
        errors.add("max_duration_minutes", FieldErrorCode::InvalidValue, None);
    }
    let elements = entries(
        &args.excluded_elements,
        "excluded_elements",
        MAX_ELEMENTS,
        is_selector,
        &mut errors,
    );
    let paths = entries(
        &args.excluded_paths,
        "excluded_paths",
        MAX_PATHS,
        is_path_pattern,
        &mut errors,
    );
    errors.finish()?;
    Ok(RecordingPolicy {
        replay_enabled: args.replay_enabled,
        heatmap_enabled: args.heatmap_enabled,
        replay_sample_percent: args.replay_sample_percent,
        heatmap_sample_percent: args.heatmap_sample_percent,
        max_duration_minutes: args.max_duration_minutes,
        excluded_elements: elements,
        excluded_paths: paths,
        version,
    })
}

fn entries(
    values: &[String],
    field: &str,
    max: usize,
    valid: fn(&str) -> bool,
    errors: &mut FieldErrors,
) -> Vec<String> {
    let mut list: Vec<String> = values
        .iter()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect();
    list.dedup();
    if list.len() > max {
        errors.add(field, FieldErrorCode::TooMany, Some(max as u32));
    } else if list
        .iter()
        .any(|value| value.chars().count() > MAX_ENTRY_CHARS)
    {
        errors.add(field, FieldErrorCode::TooLong, Some(MAX_ENTRY_CHARS as u32));
    } else if !list.iter().all(|value| valid(value)) {
        errors.add(field, FieldErrorCode::InvalidFormat, None);
    }
    list
}

/// The recorder passes the joined list to rrweb's `blockSelector`. Braces and semicolons would
/// end the selector and start a rule; the browser rejects other mistakes on its own.
fn is_selector(value: &str) -> bool {
    !value
        .chars()
        .any(|c| c.is_control() || matches!(c, '{' | '}' | ';'))
}

fn is_path_pattern(value: &str) -> bool {
    value.starts_with('/') && !value.chars().any(|c| c.is_whitespace() || c.is_control())
}

pub async fn store(
    conn: &mut SqliteConnection,
    site: &str,
    policy: &RecordingPolicy,
    now_ms: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO recording_policies (site_id, replay_enabled, heatmap_enabled, replay_sample_percent,
             heatmap_sample_percent, max_duration_minutes, excluded_elements, excluded_paths, version, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (site_id) DO UPDATE SET replay_enabled = excluded.replay_enabled,
             heatmap_enabled = excluded.heatmap_enabled, replay_sample_percent = excluded.replay_sample_percent,
             heatmap_sample_percent = excluded.heatmap_sample_percent,
             max_duration_minutes = excluded.max_duration_minutes, excluded_elements = excluded.excluded_elements,
             excluded_paths = excluded.excluded_paths, version = excluded.version, updated_at = excluded.updated_at",
    )
    .bind(site)
    .bind(policy.replay_enabled)
    .bind(policy.heatmap_enabled)
    .bind(i64::from(policy.replay_sample_percent))
    .bind(i64::from(policy.heatmap_sample_percent))
    .bind(i64::from(policy.max_duration_minutes))
    .bind(json!(policy.excluded_elements).to_string())
    .bind(json!(policy.excluded_paths).to_string())
    .bind(policy.version)
    .bind(now_ms)
    .execute(conn)
    .await?;
    Ok(())
}

/// The `GET /api/websites/{id}/recorder` body. Masking is always `strict`, so text and inputs
/// are masked in the browser before anything is sent.
pub fn recorder_config(policy: &RecordingPolicy) -> Value {
    if !policy.replay_enabled && !policy.heatmap_enabled {
        return json!({ "enabled": false });
    }
    json!({
        "enabled": true,
        "replayEnabled": policy.replay_enabled,
        "heatmapEnabled": policy.heatmap_enabled,
        "sampleRate": rate(policy.replay_sample_percent),
        "heatmapSampleRate": rate(policy.heatmap_sample_percent),
        "maskLevel": "strict",
        "maxDuration": u64::from(policy.max_duration_minutes) * 60_000,
        "blockSelector": policy.excluded_elements.join(", "),
    })
}

/// 100 percent is the integer 1, as upstream sends it.
fn rate(percent: u8) -> Value {
    if percent == 100 {
        json!(1)
    } else {
        json!(f64::from(percent) / 100.0)
    }
}

/// Whether `path` matches one of the patterns. `*` matches any run of characters; everything
/// else matches itself, and a pattern covers the whole path.
pub fn excluded(patterns: &[String], path: &str) -> bool {
    patterns
        .iter()
        .any(|pattern| glob(pattern.as_bytes(), path.as_bytes()))
}

fn glob(pattern: &[u8], text: &[u8]) -> bool {
    let (mut p, mut t) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        if p < pattern.len() && pattern[p] == b'*' {
            star = Some((p, t));
            p += 1;
        } else if p < pattern.len() && pattern[p] == text[t] {
            p += 1;
            t += 1;
        } else if let Some((sp, st)) = star {
            p = sp + 1;
            t = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|&c| c == b'*')
}

#[cfg(test)]
mod tests {
    use holocron_protocol::ids::SiteId;

    use super::*;

    #[test]
    fn patterns_cover_whole_paths() {
        let patterns = ["/account*".to_owned(), "/checkout/*/pay".to_owned()];
        assert!(excluded(&patterns, "/account"));
        assert!(excluded(&patterns, "/account/settings"));
        assert!(excluded(&patterns, "/checkout/42/pay"));
        assert!(!excluded(&patterns, "/checkout/42/pay/done"));
        assert!(!excluded(&patterns, "/my/account"));
    }

    #[test]
    fn enabled_policy_answers_strict_masking_and_default_sampling() {
        let mut policy = disabled();
        assert_eq!(recorder_config(&policy), json!({ "enabled": false }));
        policy.replay_enabled = true;
        policy.excluded_elements = vec![".private".into(), "[data-secret]".into()];
        let config = recorder_config(&policy);
        assert_eq!(config["maskLevel"], "strict");
        assert_eq!(config["sampleRate"], 0.1);
        assert_eq!(config["heatmapEnabled"], false);
        assert_eq!(config["maxDuration"], 300_000);
        assert_eq!(config["blockSelector"], ".private, [data-secret]");
    }

    #[test]
    fn validation_refuses_out_of_range_and_rule_injection() {
        let args = RecordingPolicyUpdateArgs {
            site: SiteId::new("s"),
            replay_enabled: true,
            heatmap_enabled: true,
            replay_sample_percent: 0,
            heatmap_sample_percent: 100,
            max_duration_minutes: 61,
            excluded_elements: vec![".a} body{display:none".into()],
            excluded_paths: vec!["account".into()],
            expected_revision: None,
        };
        let error = validate(&args, 1).unwrap_err();
        let fields = error.body().errors.clone().unwrap();
        let names: Vec<&str> = fields.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            [
                "excluded_elements",
                "excluded_paths",
                "max_duration_minutes",
                "replay_sample_percent"
            ]
        );
    }
}

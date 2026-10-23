//! The instant a seed treats as now, shared by the seed examples.

/// Takes `--now <RFC 3339 instant>` out of the arguments, so a seed matches a server started
/// with `--frozen-clock` at the same instant. Without it, now is the system time. Returns now
/// and the other arguments in order.
pub fn from_args() -> anyhow::Result<(i64, Vec<String>)> {
    let mut now = None;
    let mut rest = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--now" {
            let instant = args
                .next()
                .ok_or_else(|| anyhow::anyhow!("--now needs an RFC 3339 instant"))?;
            let at = chrono::DateTime::parse_from_rfc3339(&instant)
                .map_err(|e| anyhow::anyhow!("--now {instant}: {e}"))?;
            now = Some(at.timestamp_millis());
        } else {
            rest.push(arg);
        }
    }
    Ok((now.unwrap_or_else(holocron_server::db::now_ms), rest))
}

//! Runtime settings, from command-line flags or `HOLOCRON_*` environment variables.

use std::net::SocketAddr;
use std::path::PathBuf;

use axum::http::HeaderName;
use clap::{Args, ValueEnum};
use holocron_storage::Budgets;
use ipnet::IpNet;

use crate::clock::Clock;

#[derive(Debug, Clone, Args)]
pub struct ServeArgs {
    /// Persistent data directory created by `holocron init`.
    #[arg(long, env = "HOLOCRON_DATA_DIR", default_value = "data")]
    pub data_dir: PathBuf,

    /// Address to listen on.
    #[arg(long, env = "HOLOCRON_BIND", default_value = "127.0.0.1:8080")]
    pub bind: SocketAddr,

    /// Built web app to serve. Defaults to a `web` directory next to the executable.
    #[arg(long, env = "HOLOCRON_WEB_DIR")]
    pub web_dir: Option<PathBuf>,

    /// Public URL the dashboard is reached at, e.g. `https://analytics.example.com`. Commands
    /// must come from this origin, and an https URL marks the session cookie `Secure`. Without
    /// it, commands must come from the origin named by the request's Host header.
    #[arg(long, env = "HOLOCRON_PUBLIC_URL")]
    pub public_url: Option<String>,

    /// Name of this installation, shown in the dashboard.
    #[arg(long, env = "HOLOCRON_INSTANCE_NAME", default_value = "Holocron")]
    pub instance_name: String,

    /// Setup token for an empty data directory, for scripted installs. Without it, setup mode
    /// generates one at each start and logs it (decision 0021).
    #[arg(long, env = "HOLOCRON_SETUP_TOKEN", hide_env_values = true)]
    pub setup_token: Option<String>,

    /// Directory with `holocron-backup-*` backups that the setup wizard offers to restore. The
    /// compose files mount the backup storage at `/backups`.
    #[arg(long, env = "HOLOCRON_BACKUP_DIR", default_value = "/backups")]
    pub backup_dir: PathBuf,

    /// Free space the data volume keeps, in MiB. Below it collection answers 503 so the
    /// server can still shut down cleanly (decision 0008).
    #[arg(long, env = "HOLOCRON_DISK_RESERVE_MIB", default_value_t = 4096)]
    pub disk_reserve_mib: u64,

    /// Upgrades data from an older release when the server starts, after a verified backup into
    /// HOLOCRON_BACKUP_DIR (decision 0006, Amendment). Without it, older data is refused with
    /// `upgrade_required`. Data from a newer release is always refused.
    #[arg(long, env = "HOLOCRON_UPGRADE_ON_START", action = clap::ArgAction::SetTrue, value_parser = clap::builder::FalseyValueParser::new())]
    pub upgrade_on_start: bool,

    /// Resource preset for the host size: engine threads and memory, Tokio workers, report
    /// workers and queue, spill and inbox. `small` and `large` are estimates, not measured
    /// (decisions 0008 and 0019). The server never picks one on its own.
    #[arg(long, env = "HOLOCRON_BUDGET", value_enum, default_value_t = Budget::Standard)]
    pub budget: Budget,

    /// Tokio worker threads. Overrides the preset's value.
    #[arg(long, env = "HOLOCRON_WORKER_THREADS")]
    pub worker_threads: Option<usize>,

    /// Reverse proxies whose forwarding and geo headers are trusted: comma-separated IPs or
    /// CIDRs. Empty means the connection's peer address is the client.
    #[arg(long, env = "HOLOCRON_COLLECT_TRUSTED_PROXIES", value_delimiter = ',')]
    pub collect_trusted_proxies: Vec<String>,

    /// Header a trusted proxy puts the client address in.
    #[arg(
        long,
        env = "HOLOCRON_COLLECT_CLIENT_IP_HEADER",
        default_value = "x-forwarded-for"
    )]
    pub collect_client_ip_header: String,

    /// Client addresses whose events are not stored (upstream `IGNORE_IP`): comma-separated
    /// IPs or CIDRs.
    #[arg(long, env = "HOLOCRON_COLLECT_IGNORE_IPS", value_delimiter = ',')]
    pub collect_ignore_ips: Vec<String>,

    /// Extra names the tracker is served under (upstream `TRACKER_SCRIPT_NAME`), comma-separated.
    #[arg(long, env = "HOLOCRON_TRACKER_SCRIPT_NAME", value_delimiter = ',')]
    pub tracker_script_names: Vec<String>,

    /// Extra path for `/api/send` (upstream `COLLECT_API_ENDPOINT`). The served tracker is
    /// rewritten to post there.
    #[arg(long, env = "HOLOCRON_COLLECT_API_ENDPOINT")]
    pub collect_endpoint: Option<String>,

    /// MaxMind DB City file (GeoLite2, GeoIP2 or DB-IP Lite City) that locates visitors no
    /// trusted CDN header located. Read at start. While set, the dashboard download is off
    /// (decision 0018).
    #[arg(long, env = "HOLOCRON_GEOIP_DB")]
    pub geoip_db: Option<PathBuf>,

    /// Compressed replay storage, in MiB. Beyond it new replay data is refused while every other
    /// collection continues. Saved replays may hold a fifth of it.
    #[arg(long, env = "HOLOCRON_RECORDING_CAP_MIB", default_value_t = 25 * 1024)]
    pub recording_cap_mib: u64,

    /// Runs on an application clock frozen at this RFC 3339 instant, e.g.
    /// `2026-10-13T15:20:00+02:00`, for browser journeys on seeded data. Only builds with the
    /// `dev-clock` feature accept it; see [`clock`].
    #[arg(long, env = "HOLOCRON_FROZEN_CLOCK")]
    pub frozen_clock: Option<String>,
}

/// Installation tier from decision 0008, chosen by the operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Budget {
    /// 1 vCPU, 2 GiB. Estimated.
    Small,
    /// 2 vCPU, 4 GiB. The reference host, measured.
    Standard,
    /// 4 vCPU, 8 GiB. Estimated.
    Large,
}

/// What a tier and the explicit flags resolve to.
#[derive(Debug, Clone)]
pub struct Sizing {
    pub budget: Budget,
    pub worker_threads: usize,
    pub storage: Budgets,
}

impl Budget {
    /// The table of decision 0008. Spill and inbox are not in it and stay at the standard
    /// values, which are the storage default.
    fn sizing(self) -> Sizing {
        const MIB: u64 = 1024 * 1024;
        let (threads, memory_mib, report_queue) = match self {
            Budget::Small => (1, 512, 4),
            Budget::Standard => (2, 1024, 8),
            Budget::Large => (4, 2048, 8),
        };
        Sizing {
            budget: self,
            worker_threads: threads as usize,
            storage: Budgets {
                engine_threads: threads,
                engine_memory_bytes: memory_mib * MIB,
                report_workers: threads as usize,
                report_queue,
                ..Budgets::default()
            },
        }
    }
}

impl ServeArgs {
    /// The preset with the explicit flags applied over it.
    pub fn sizing(&self) -> Sizing {
        let mut sizing = self.budget.sizing();
        if let Some(threads) = self.worker_threads {
            sizing.worker_threads = threads.max(1);
        }
        sizing.storage.disk_reserve_bytes = self.disk_reserve_mib * 1024 * 1024;
        sizing
    }
}

/// The clock `serve` runs on: the system clock, or one frozen at `frozen`. A build without the
/// `dev-clock` feature refuses a frozen clock, so a packaged release always runs on real time.
pub fn clock(frozen: Option<&str>) -> anyhow::Result<Clock> {
    let Some(instant) = frozen else {
        return Ok(Clock::system());
    };
    anyhow::ensure!(
        cfg!(feature = "dev-clock"),
        "this build does not support a frozen clock (--frozen-clock, HOLOCRON_FROZEN_CLOCK); build with the dev-clock feature"
    );
    let at = chrono::DateTime::parse_from_rfc3339(instant.trim())
        .map_err(|e| anyhow::anyhow!("--frozen-clock {instant}: not an RFC 3339 instant: {e}"))?;
    Ok(Clock::frozen(at.timestamp_millis()))
}

/// Public collection settings (decisions 0001 Proxy trust, 0005 Operator IP block list).
#[derive(Debug, Clone)]
pub struct CollectConfig {
    pub trusted_proxies: Vec<IpNet>,
    pub client_ip_header: HeaderName,
    pub ignore_ips: Vec<IpNet>,
    /// Paths without a leading slash.
    pub tracker_script_names: Vec<String>,
    pub collect_endpoint: Option<String>,
    /// Request body bytes read but not yet answered, across all collection requests. Beyond
    /// it requests are refused with 429 instead of growing memory.
    pub intake_bytes: usize,
    /// Replay storage cap (CAP-RECORD-06), default 25 GiB.
    pub recording_cap_bytes: u64,
    /// The operator's GeoIP database, which turns the download off (decision 0018).
    pub geoip_db: Option<PathBuf>,
    /// Where monthly DB-IP Lite City files are published. Tests point it at a closed port.
    pub geoip_download: String,
}

impl Default for CollectConfig {
    fn default() -> Self {
        Self {
            trusted_proxies: Vec::new(),
            client_ip_header: HeaderName::from_static("x-forwarded-for"),
            ignore_ips: Vec::new(),
            tracker_script_names: Vec::new(),
            collect_endpoint: None,
            intake_bytes: 32 * 1024 * 1024,
            recording_cap_bytes: crate::recordings::DEFAULT_CAP_BYTES,
            geoip_db: None,
            geoip_download: "https://download.db-ip.com/free".into(),
        }
    }
}

/// `203.0.113.7` or `10.0.0.0/8`.
pub fn parse_net(value: &str) -> anyhow::Result<IpNet> {
    let value = value.trim();
    value
        .parse::<IpNet>()
        .or_else(|_| value.parse::<std::net::IpAddr>().map(IpNet::from))
        .map_err(|_| anyhow::anyhow!("{value} is not an IP address or CIDR"))
}

#[derive(Debug, Clone)]
pub struct Config {
    pub data_dir: PathBuf,
    pub bind: SocketAddr,
    pub web_dir: Option<PathBuf>,
    /// `scheme://host[:port]` without a trailing slash.
    pub public_origin: Option<String>,
    pub instance_name: String,
    pub setup_token: Option<String>,
    /// Where the setup wizard looks for backups to restore.
    pub backup_dir: PathBuf,
    pub collect: CollectConfig,
    pub disk_reserve_bytes: u64,
}

impl Config {
    pub fn from_args(args: ServeArgs) -> anyhow::Result<Self> {
        let public_origin = args.public_url.as_deref().map(origin_of).transpose()?;
        let web_dir = args.web_dir.or_else(|| {
            let beside = std::env::current_exe().ok()?.parent()?.join("web");
            beside.join("index.html").is_file().then_some(beside)
        });
        let nets = |values: &[String]| -> anyhow::Result<Vec<IpNet>> {
            values
                .iter()
                .filter(|v| !v.trim().is_empty())
                .map(|v| parse_net(v))
                .collect()
        };
        let collect_endpoint = args
            .collect_endpoint
            .map(|path| path.trim().to_owned())
            .filter(|path| !path.is_empty());
        if let Some(path) = &collect_endpoint {
            anyhow::ensure!(
                path.starts_with('/') && path != "/api/send",
                "HOLOCRON_COLLECT_API_ENDPOINT must be a path such as /api/collect"
            );
        }
        let collect = CollectConfig {
            trusted_proxies: nets(&args.collect_trusted_proxies)?,
            client_ip_header: HeaderName::try_from(
                args.collect_client_ip_header.trim().to_lowercase(),
            )
            .map_err(|_| {
                anyhow::anyhow!("HOLOCRON_COLLECT_CLIENT_IP_HEADER is not a header name")
            })?,
            ignore_ips: nets(&args.collect_ignore_ips)?,
            tracker_script_names: args
                .tracker_script_names
                .iter()
                .map(|name| name.trim().trim_start_matches('/').to_owned())
                .filter(|name| !name.is_empty())
                .collect(),
            collect_endpoint,
            recording_cap_bytes: args.recording_cap_mib * 1024 * 1024,
            geoip_db: args.geoip_db.filter(|path| !path.as_os_str().is_empty()),
            ..CollectConfig::default()
        };
        Ok(Self {
            data_dir: args.data_dir,
            bind: args.bind,
            web_dir,
            public_origin,
            instance_name: args.instance_name,
            setup_token: args.setup_token,
            backup_dir: args.backup_dir,
            collect,
            disk_reserve_bytes: args.disk_reserve_mib * 1024 * 1024,
        })
    }

    pub fn secure_cookies(&self) -> bool {
        self.public_origin
            .as_deref()
            .is_some_and(|origin| origin.starts_with("https://"))
    }
}

/// `https://example.com/analytics/` → `https://example.com`.
fn origin_of(url: &str) -> anyhow::Result<String> {
    let url = url.trim();
    let (scheme, rest) = url
        .split_once("://")
        .filter(|(scheme, _)| matches!(*scheme, "http" | "https"))
        .ok_or_else(|| {
            anyhow::anyhow!("HOLOCRON_PUBLIC_URL must start with http:// or https://")
        })?;
    let host = rest.split('/').next().unwrap_or_default();
    anyhow::ensure!(!host.is_empty(), "HOLOCRON_PUBLIC_URL has no host");
    Ok(format!("{scheme}://{}", host.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Parser)]
    struct Cli {
        #[command(flatten)]
        args: ServeArgs,
    }

    fn sizing(flags: &[&str]) -> Sizing {
        Cli::try_parse_from(["holocron"].into_iter().chain(flags.iter().copied()))
            .unwrap()
            .args
            .sizing()
    }

    #[test]
    fn presets_follow_decision_0008() {
        let small = sizing(&["--budget", "small"]);
        assert_eq!((small.worker_threads, small.storage.engine_threads), (1, 1));
        assert_eq!(small.storage.report_queue, 4);
        assert_eq!(small.storage.engine_memory_bytes, 512 << 20);
        let large = sizing(&["--budget", "large"]);
        assert_eq!((large.worker_threads, large.storage.report_workers), (4, 4));
        assert_eq!(large.storage.engine_memory_bytes, 2048 << 20);
        let standard = sizing(&[]);
        assert_eq!(standard.budget, Budget::Standard);
        assert_eq!(standard.worker_threads, 2);
        assert_eq!(
            standard.storage.engine_memory_bytes,
            Budgets::default().engine_memory_bytes
        );
    }

    #[test]
    fn an_explicit_flag_overrides_the_preset() {
        let sizing = sizing(&["--budget", "large", "--worker-threads", "3"]);
        assert_eq!(sizing.worker_threads, 3);
        assert_eq!(sizing.storage.engine_threads, 4);
    }
}

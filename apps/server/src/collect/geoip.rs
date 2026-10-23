//! The GeoIP fallback (decision 0018): a MaxMind DB City database that locates visitors no
//! trusted CDN header located. It is the operator's file from `HOLOCRON_GEOIP_DB`, or DB-IP Lite
//! City, which install admins download into `<data>/geoip/`. A download is verified before it
//! replaces the database lookups use, without a restart. A missing or broken file only means
//! no location; it never fails collection. Addresses stay in request memory.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::Duration;

use chrono::{DateTime, Datelike, Days};
use flate2::read::GzDecoder;
use holocron_protocol::envelope::{Empty, Topic};
use holocron_protocol::operations::geoip::{
    GeoipDatabase, GeoipDownload, GeoipFailure, GeoipHandlers, GeoipSettings,
};
use holocron_protocol::registry::Snapshot;
use maxminddb::{Mmap, Reader, geoip2};

use super::client::{Geo, coordinates, region_code};
use crate::config::CollectConfig;
use crate::context::{CommandCtx, QueryCtx};
use crate::error::ApiError;
use crate::state::AppState;

/// Downloaded databases, beside the engine files. Not part of a backup.
pub const GEOIP_DIR: &str = "geoip";
const PREFIX: &str = "dbip-city-lite-";
const SUFFIX: &str = ".mmdb";
const PARTIAL: &str = ".partial";
/// Unpacked size a download may reach, and the free space it needs above the disk reserve.
/// The October 2026 file is 127 MB.
pub const UNPACKED_LIMIT: u64 = 512 * 1024 * 1024;
/// How long the newest published month is remembered.
const LATEST_TTL_MS: i64 = 60 * 60 * 1000;

/// A database file's bytes: the operator's file is copied into memory, because it may be
/// overwritten in place; a downloaded one is mapped, because Holocron never writes it again.
enum Bytes {
    Heap(Vec<u8>),
    Mapped(Mmap),
}

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        match self {
            Bytes::Heap(bytes) => bytes,
            Bytes::Mapped(map) => map,
        }
    }
}

struct Database {
    reader: Reader<Bytes>,
    /// `YYYY-MM` of a downloaded file.
    month: Option<String>,
}

impl Database {
    /// Accepts only a City database that passes the reader's full verification.
    fn new(bytes: Bytes, month: Option<String>) -> Result<Self, String> {
        let reader = Reader::from_source(bytes).map_err(|e| e.to_string())?;
        let kind = &reader.metadata().database_type;
        if !kind.contains("City") {
            return Err(format!("{kind} is not a City database"));
        }
        reader.verify().map_err(|e| e.to_string())?;
        Ok(Self { reader, month })
    }

    fn info(&self) -> GeoipDatabase {
        let meta = self.reader.metadata();
        GeoipDatabase {
            database_type: meta.database_type.clone(),
            built_at: i64::try_from(meta.build_epoch).unwrap_or(i64::MAX / 1000) * 1000,
            month: self.month.clone(),
            dbip: meta.database_type.starts_with("DBIP"),
        }
    }

    fn lookup(&self, ip: IpAddr) -> Option<Geo> {
        let record = self
            .reader
            .lookup(ip)
            .ok()?
            .decode::<geoip2::City>()
            .ok()??;
        let country = record
            .country
            .iso_code
            .or(record.registered_country.iso_code)?;
        let country: String = country.to_ascii_uppercase().chars().take(2).collect();
        let region = record
            .subdivisions
            .first()
            .and_then(|subdivision| subdivision.iso_code)
            .map(|code| region_code(&country, code));
        let city = record
            .city
            .names
            .english
            .filter(|name| !name.is_empty())
            .map(|name| name.chars().take(50).collect());
        let location = &record.location;
        let (latitude, longitude) = location
            .latitude
            .zip(location.longitude)
            .and_then(|(lat, lng)| coordinates(lat, lng))
            .unzip();
        Some(Geo {
            country: Some(country),
            region,
            city,
            latitude,
            longitude,
        })
    }
}

pub struct GeoIp {
    dir: PathBuf,
    external: bool,
    download_base: String,
    current: RwLock<Option<Database>>,
    download: Mutex<GeoipDownload>,
    /// When the newest published month was looked up, and the answer.
    latest: Mutex<Option<(i64, Option<String>)>>,
}

impl GeoIp {
    /// Loads the operator's file, or the newest downloaded month, and removes partial downloads.
    /// Reads and verifies the whole file, so call it off the async workers.
    pub fn open(data_dir: &Path, config: &CollectConfig) -> Self {
        let dir = data_dir.join(GEOIP_DIR);
        let current = match &config.geoip_db {
            Some(path) => fs::read(path)
                .map_err(|e| e.to_string())
                .and_then(|bytes| Database::new(Bytes::Heap(bytes), None))
                .inspect_err(|error| {
                    tracing::warn!(path = %path.display(), %error, "HOLOCRON_GEOIP_DB is not usable; visitors without a CDN location get none");
                })
                .ok(),
            None => installed(&dir),
        };
        Self {
            dir,
            external: config.geoip_db.is_some(),
            download_base: config.geoip_download.trim_end_matches('/').to_owned(),
            current: RwLock::new(current),
            download: Mutex::new(GeoipDownload::Idle),
            latest: Mutex::new(None),
        }
    }

    /// The location of `ip`; empty without a database or a record.
    pub fn lookup(&self, ip: IpAddr) -> Geo {
        let current = self.current.read().unwrap_or_else(PoisonError::into_inner);
        current
            .as_ref()
            .and_then(|db| db.lookup(ip))
            .unwrap_or_default()
    }

    /// Whether DB-IP's licence asks for its link (0018 Attribution).
    pub fn dbip(&self) -> bool {
        self.database().is_some_and(|db| db.dbip)
    }

    fn database(&self) -> Option<GeoipDatabase> {
        let current = self.current.read().unwrap_or_else(PoisonError::into_inner);
        current.as_ref().map(Database::info)
    }

    /// The settings without contacting DB-IP: the newest month only if it is remembered.
    pub fn snapshot(&self, now_ms: i64) -> GeoipSettings {
        let database = self.database();
        let latest = self.latest.lock().unwrap_or_else(PoisonError::into_inner);
        let latest_month = match &*latest {
            Some((at, month))
                if now_ms - at < LATEST_TTL_MS && self.outdated(&database, now_ms) =>
            {
                month.clone()
            }
            _ => None,
        };
        GeoipSettings {
            database,
            external: self.external,
            latest_month,
            download: self
                .download
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
        }
    }

    /// [`GeoIp::snapshot`], after asking DB-IP for its newest month when the installed one is
    /// older than the current month and the last answer is over an hour old.
    pub async fn settings(self: &Arc<Self>, now_ms: i64) -> GeoipSettings {
        let fresh = self
            .latest
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_some_and(|(at, _)| now_ms - at < LATEST_TTL_MS);
        if !fresh && self.outdated(&self.database(), now_ms) {
            let geoip = Arc::clone(self);
            let month = tokio::task::spawn_blocking(move || geoip.latest_month(now_ms))
                .await
                .unwrap_or_default();
            *self.latest.lock().unwrap_or_else(PoisonError::into_inner) = Some((now_ms, month));
        }
        self.snapshot(now_ms)
    }

    /// A downloaded month older than the current one is installed.
    fn outdated(&self, database: &Option<GeoipDatabase>, now_ms: i64) -> bool {
        let [current, _] = months(now_ms);
        !self.external
            && database
                .as_ref()
                .and_then(|db| db.month.as_deref())
                .is_some_and(|month| month < current.as_str())
    }

    /// Starts a download in the background unless one runs or the operator's file is in use.
    /// `room` is the free space above the disk reserve.
    pub fn start_download(self: &Arc<Self>, now_ms: i64, room: u64) {
        let mut state = self.download.lock().unwrap_or_else(PoisonError::into_inner);
        if self.external || *state == GeoipDownload::Running {
            return;
        }
        if room < UNPACKED_LIMIT {
            *state = GeoipDownload::Failed {
                reason: GeoipFailure::DiskFull,
            };
            return;
        }
        *state = GeoipDownload::Running;
        let geoip = Arc::clone(self);
        tokio::task::spawn_blocking(move || {
            let outcome = geoip.download(now_ms);
            *geoip
                .download
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = match outcome {
                Ok(()) => GeoipDownload::Idle,
                Err(reason) => GeoipDownload::Failed { reason },
            };
        });
    }

    /// Fetches this month's file, or last month's before DB-IP publishes the new one.
    fn download(&self, now_ms: i64) -> Result<(), GeoipFailure> {
        let agent = agent(Duration::from_secs(600));
        for month in months(now_ms) {
            let url = format!("{}/{PREFIX}{month}.mmdb.gz", self.download_base);
            match agent.get(&url).call() {
                Ok(response) => {
                    return self.install(&month, response.into_body().into_reader());
                }
                Err(ureq::Error::StatusCode(404)) => continue,
                Err(error) => {
                    tracing::warn!(%error, "GeoIP download failed");
                    return Err(GeoipFailure::Unreachable);
                }
            }
        }
        Err(GeoipFailure::NotPublished)
    }

    /// Unpacks the gzip `packed` into a temporary file, verifies it, renames it into place and
    /// swaps it in. Other months are removed. On failure the installed database stays.
    pub(crate) fn install(&self, month: &str, packed: impl Read) -> Result<(), GeoipFailure> {
        let mut source = GzDecoder::new(packed);
        let written = |error: io::Error| {
            tracing::warn!(%error, "GeoIP file could not be written");
            GeoipFailure::DiskFull
        };
        fs::create_dir_all(&self.dir).map_err(written)?;
        let name = format!("{PREFIX}{month}{SUFFIX}");
        let partial = self.dir.join(format!("{name}{PARTIAL}"));
        let result = (|| {
            let mut file = File::create(&partial).map_err(written)?;
            let mut buffer = vec![0; 256 * 1024];
            let mut total = 0;
            loop {
                let read = match source.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => read,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => {
                        tracing::warn!(%error, "GeoIP download broke off");
                        return Err(match error.kind() {
                            io::ErrorKind::InvalidData | io::ErrorKind::InvalidInput => {
                                GeoipFailure::Invalid
                            }
                            _ => GeoipFailure::Unreachable,
                        });
                    }
                };
                total += read as u64;
                if total > UNPACKED_LIMIT {
                    return Err(GeoipFailure::Invalid);
                }
                file.write_all(&buffer[..read]).map_err(written)?;
            }
            file.sync_all().map_err(written)?;
            let database = map(&partial, month).map_err(|error| {
                tracing::warn!(%error, "downloaded GeoIP file is not usable");
                GeoipFailure::Invalid
            })?;
            fs::rename(&partial, self.dir.join(&name)).map_err(written)?;
            Ok(database)
        })();
        let database = result.inspect_err(|_| {
            let _ = fs::remove_file(&partial);
        })?;
        *self.current.write().unwrap_or_else(PoisonError::into_inner) = Some(database);
        // The replaced file stays readable through its mapping until the swap drops it.
        for entry in fs::read_dir(&self.dir).into_iter().flatten().flatten() {
            let other = entry.file_name();
            if other
                .to_str()
                .is_some_and(|other| other.starts_with(PREFIX) && other != name)
            {
                let _ = fs::remove_file(entry.path());
            }
        }
        Ok(())
    }

    /// The newest month DB-IP has published, `None` when it does not answer.
    fn latest_month(&self, now_ms: i64) -> Option<String> {
        let agent = agent(Duration::from_secs(5));
        for month in months(now_ms) {
            let url = format!("{}/{PREFIX}{month}.mmdb.gz", self.download_base);
            match agent.head(&url).call() {
                Ok(_) => return Some(month),
                Err(ureq::Error::StatusCode(404)) => continue,
                Err(_) => return None,
            }
        }
        None
    }
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(Some(timeout))
            .user_agent(concat!("holocron/", env!("CARGO_PKG_VERSION")))
            .build(),
    )
}

/// The current UTC month and the one before, as `YYYY-MM`.
fn months(now_ms: i64) -> [String; 2] {
    let today = DateTime::from_timestamp_millis(now_ms)
        .unwrap_or_default()
        .date_naive();
    let first = today.with_day(1).expect("every month has a first day");
    let previous = first - Days::new(1);
    [first, previous].map(|day| day.format("%Y-%m").to_string())
}

/// The newest downloaded month that opens, after removing partial downloads.
fn installed(dir: &Path) -> Option<Database> {
    let mut months: Vec<String> = Vec::new();
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.ends_with(PARTIAL) {
            let _ = fs::remove_file(entry.path());
        } else if let Some(month) = name
            .strip_prefix(PREFIX)
            .and_then(|rest| rest.strip_suffix(SUFFIX))
        {
            months.push(month.to_owned());
        }
    }
    let month = months.into_iter().max()?;
    map(&dir.join(format!("{PREFIX}{month}{SUFFIX}")), &month)
        .inspect_err(|error| tracing::warn!(%error, "installed GeoIP file is not usable"))
        .ok()
}

fn map(path: &Path, month: &str) -> Result<Database, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    // SAFETY: files in the GeoIP directory are written once under a temporary name and never
    // modified after the rename; a newer month is a new file.
    let map = unsafe { Mmap::map(&file) }.map_err(|e| e.to_string())?;
    Database::new(Bytes::Mapped(map), Some(month.to_owned()))
}

impl GeoipHandlers for AppState {
    async fn geoip_download(
        &self,
        ctx: &mut CommandCtx,
        _args: Empty,
    ) -> Result<GeoipSettings, ApiError> {
        ctx.require_install_admin().await?;
        let c = &self.collector;
        let (free, reserve) = c.storage().disk_headroom().map_err(ApiError::internal)?;
        let now = c.clock().now_ms();
        c.geoip().start_download(now, free.saturating_sub(reserve));
        Ok(c.geoip().snapshot(now))
    }

    async fn geoip_settings(
        &self,
        ctx: &mut QueryCtx,
        _args: Empty,
    ) -> Result<Snapshot<GeoipSettings>, ApiError> {
        ctx.require_install_admin().await?;
        let now = self.collector.clock().now_ms();
        Ok(Snapshot {
            topic: Topic::installation(),
            data: self.collector.geoip().settings(now).await,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;

    /// MaxMind's GeoIP2 City test database (MIT or Apache-2.0, see fixtures/geoip).
    fn fixture() -> Vec<u8> {
        fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/geoip/GeoIP2-City-Test.mmdb"),
        )
        .unwrap()
    }

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn a_verified_download_replaces_the_database_and_a_broken_one_does_not() {
        let data = tempfile::tempdir().unwrap();
        let geoip = GeoIp::open(data.path(), &CollectConfig::default());
        let london: IpAddr = "81.2.69.142".parse().unwrap();
        assert_eq!(geoip.lookup(london), Geo::default());

        geoip
            .install("2026-09", gzip(&fixture()).as_slice())
            .unwrap();
        assert_eq!(geoip.lookup(london).country.as_deref(), Some("GB"));

        let truncated = fixture()[..4096].to_vec();
        assert_eq!(
            geoip.install("2026-10", gzip(&truncated).as_slice()),
            Err(GeoipFailure::Invalid)
        );
        assert_eq!(geoip.lookup(london).country.as_deref(), Some("GB"));
        assert_eq!(
            geoip.snapshot(0).database.unwrap().month.as_deref(),
            Some("2026-09")
        );

        geoip
            .install("2026-10", gzip(&fixture()).as_slice())
            .unwrap();
        let files: Vec<_> = fs::read_dir(data.path().join(GEOIP_DIR))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(files, ["dbip-city-lite-2026-10.mmdb"]);
        let reopened = GeoIp::open(data.path(), &CollectConfig::default());
        assert_eq!(reopened.lookup(london).city.as_deref(), Some("London"));
    }
}

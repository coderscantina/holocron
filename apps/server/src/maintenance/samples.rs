//! Daily storage samples and the storage forecast (dashboard v2 v07, 0008 Budgets).
//!
//! A sample reads file sizes and DuckDB `day_aggregates` only, never events, so it stays cheap
//! at any volume. The job runner takes one at start and then daily; retention trims rows older
//! than [`KEEP_MS`].
//!
//! The forecast is a pure function of the latest sample. Retention removes data at least hourly
//! (0006), so disk use rises while less than 12 months are retained and then levels off; it
//! never follows a monthly cycle.

use chrono::Months;
use holocron_analytics::identity::{date_of_local_day, days_since_epoch};
use holocron_protocol::operations::operator::{
    DiskUsage, ForecastInputs, ForecastOutcome, StorageForecast, StorageSample,
};
use holocron_storage::Lane;
use sqlx::SqlitePool;

use crate::db::CONTROL_DB_FILE;
use crate::recordings::store::RECORDINGS_DB_FILE;
use crate::state::AppState;

pub const DAY_MS: i64 = 86_400_000;
/// A failed sample is tried again after this long.
pub const RETRY_MS: i64 = 60 * 60 * 1000;
/// Samples are kept this long.
pub const KEEP_MS: i64 = 400 * DAY_MS;
/// Days over which ingest per day is the median.
const INGEST_DAYS: i32 = 28;
/// An installation with fewer retained days gets no forecast.
const MIN_HISTORY_DAYS: i64 = 14;

/// One row of `storage_samples`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::FromRow)]
pub struct Sample {
    pub taken_at: i64,
    pub control_bytes: i64,
    pub inbox_bytes: i64,
    pub recordings_bytes: i64,
    pub analytics_bytes: i64,
    pub spill_bytes: i64,
    pub free_bytes: i64,
    pub retained_records: i64,
    pub ingest_per_day: i64,
    /// Oldest retained local date, days since 1970-01-01.
    pub oldest_day: Option<i32>,
}

impl Sample {
    pub fn disk(&self) -> DiskUsage {
        DiskUsage {
            control_bytes: self.control_bytes,
            inbox_bytes: self.inbox_bytes,
            recordings_bytes: self.recordings_bytes,
            analytics_bytes: self.analytics_bytes,
            spill_bytes: self.spill_bytes,
            free_bytes: self.free_bytes,
        }
    }

    pub fn summary(&self) -> StorageSample {
        StorageSample {
            taken_at: self.taken_at,
            used_bytes: self.disk().used_bytes(),
            free_bytes: self.free_bytes,
            retained_records: self.retained_records,
        }
    }
}

/// Sizes of the engine files and free space now. Reads file metadata only.
pub fn disk(state: &AppState) -> std::io::Result<DiskUsage> {
    let dir = &state.config.data_dir;
    let with_wal = |name: &str| {
        [name.to_owned(), format!("{name}-wal")]
            .iter()
            .map(|file| std::fs::metadata(dir.join(file)).map_or(0, |m| m.len()))
            .sum::<u64>()
    };
    let engines = state.collector.storage().disk()?;
    let bytes = |value: u64| value.min(i64::MAX as u64) as i64;
    Ok(DiskUsage {
        control_bytes: bytes(with_wal(CONTROL_DB_FILE)),
        inbox_bytes: bytes(engines.inbox_bytes + engines.inbox_wal_bytes),
        recordings_bytes: bytes(with_wal(RECORDINGS_DB_FILE)),
        analytics_bytes: bytes(engines.analytics_bytes + engines.analytics_wal_bytes),
        spill_bytes: bytes(engines.spill_bytes),
        free_bytes: bytes(engines.free_bytes),
    })
}

/// Takes a sample at the application clock's now and stores it.
pub async fn take(state: &AppState) -> anyhow::Result<Sample> {
    let now = state.collector.clock().now_ms();
    let today = utc_day(now);
    let disk = disk(state)?;
    let (retained_records, oldest_day, daily) = state
        .collector
        .storage()
        .read(Lane::Report, move |s| {
            let conn = s.connection();
            let (retained, oldest): (i64, Option<i32>) = conn.query_row(
                "SELECT coalesce(sum(eligible_events), 0)::BIGINT,
                        (min(local_day) - DATE '1970-01-01')::INTEGER
                 FROM day_aggregates",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let from = oldest.map_or(today, |day| day.max(today - INGEST_DAYS));
            let mut daily = conn
                .prepare(
                    "SELECT sum(eligible_events)::BIGINT FROM day_aggregates
                     WHERE local_day >= DATE '1970-01-01' + ?::INTEGER
                       AND local_day < DATE '1970-01-01' + ?::INTEGER
                     GROUP BY local_day",
                )?
                .query_map([from, today], |r| r.get::<_, i64>(0))?
                .collect::<holocron_storage::duckdb::Result<Vec<_>>>()?;
            // Days of the window without a rollup row had no eligible events.
            daily.resize((today - from).max(0) as usize, 0);
            Ok((retained, oldest, daily))
        })
        .await?
        .value;
    let sample = Sample {
        taken_at: now,
        control_bytes: disk.control_bytes,
        inbox_bytes: disk.inbox_bytes,
        recordings_bytes: disk.recordings_bytes,
        analytics_bytes: disk.analytics_bytes,
        spill_bytes: disk.spill_bytes,
        free_bytes: disk.free_bytes,
        retained_records,
        ingest_per_day: median(daily),
        oldest_day,
    };
    sqlx::query(
        "INSERT OR REPLACE INTO storage_samples (taken_at, control_bytes, inbox_bytes,
             recordings_bytes, analytics_bytes, spill_bytes, free_bytes, retained_records,
             ingest_per_day, oldest_day)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(sample.taken_at)
    .bind(sample.control_bytes)
    .bind(sample.inbox_bytes)
    .bind(sample.recordings_bytes)
    .bind(sample.analytics_bytes)
    .bind(sample.spill_bytes)
    .bind(sample.free_bytes)
    .bind(sample.retained_records)
    .bind(sample.ingest_per_day)
    .bind(sample.oldest_day)
    .execute(&state.db.writer)
    .await?;
    Ok(sample)
}

/// Every kept sample, oldest first.
pub async fn list(pool: &SqlitePool) -> sqlx::Result<Vec<Sample>> {
    sqlx::query_as("SELECT * FROM storage_samples ORDER BY taken_at")
        .fetch_all(pool)
        .await
}

/// The median of whole numbers, the lower one of the middle pair; 0 for none.
fn median(mut values: Vec<i64>) -> i64 {
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    values[(values.len() - 1) / 2]
}

fn utc_day(ms: i64) -> i32 {
    ms.div_euclid(DAY_MS) as i32
}

fn date(day: i32) -> String {
    date_of_local_day(day).format("%Y-%m-%d").to_string()
}

/// Whether the volume holds 12 months of data, from one sample, the configured replay cap and
/// the disk reserve.
///
/// Steady state is bytes per record × ingest per day × 365 + the SQLite files + the replay cap
/// + the reserve, reached once 12 months are retained. It fits when it is at most the used
///   plus free bytes. When it does not, use is taken to rise in a straight line from now to the
///   steady state, and the outcome is the day free space reaches the reserve on that line.
pub fn forecast(
    sample: &Sample,
    replay_cap_bytes: i64,
    disk_reserve_bytes: i64,
) -> StorageForecast {
    let today = utc_day(sample.taken_at);
    let disk = sample.disk();
    let used = disk.used_bytes();
    let capacity = used + sample.free_bytes;
    let sqlite_bytes = sample.control_bytes + sample.inbox_bytes;
    let records = sample.retained_records;
    let analytics_steady = if records > 0 {
        (i128::from(sample.analytics_bytes) * i128::from(sample.ingest_per_day) * 365
            / i128::from(records)) as i64
    } else {
        0
    };
    let steady = analytics_steady + sqlite_bytes + replay_cap_bytes + disk_reserve_bytes;
    let oldest = sample.oldest_day.filter(|_| records > 0);
    let full_day = oldest.map(|day| days_since_epoch(date_of_local_day(day) + Months::new(12)));
    let days_left = full_day.map(|day| i64::from(day - today).max(0));

    let outcome = match (oldest, full_day, days_left) {
        (Some(oldest), Some(full_day), Some(days_left))
            if i64::from(today - oldest) >= MIN_HISTORY_DAYS =>
        {
            if steady <= capacity {
                ForecastOutcome::LevelsOff {
                    size_bytes: steady,
                    on: date(full_day.max(today)),
                }
            } else {
                let headroom = sample.free_bytes - disk_reserve_bytes;
                let growth = steady - disk_reserve_bytes - used;
                let days = if headroom <= 0 {
                    0
                } else {
                    // steady > capacity, so growth > headroom > 0 and days ≤ days_left.
                    (headroom as u128 * days_left.max(1) as u128).div_ceil(growth as u128) as i64
                };
                ForecastOutcome::ReachesReserve {
                    on: date(today + days as i32),
                }
            }
        }
        _ => ForecastOutcome::NotEnoughHistory {
            days: oldest.map_or(0, |day| i64::from(today - day).max(0)),
        },
    };
    StorageForecast {
        inputs: ForecastInputs {
            sampled_at: sample.taken_at,
            retained_records: records,
            analytics_bytes: sample.analytics_bytes,
            bytes_per_record: if records > 0 {
                sample.analytics_bytes as f64 / records as f64
            } else {
                0.0
            },
            ingest_per_day: sample.ingest_per_day,
            oldest_day: oldest.map(date),
            full_retention_on: full_day.map(date),
            days_until_full_retention: days_left,
            sqlite_bytes,
            replay_cap_bytes,
            disk_reserve_bytes,
            steady_state_bytes: steady,
            used_bytes: used,
            capacity_bytes: capacity,
        },
        outcome,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: i64 = 1_000_000_000;
    const GIB: i64 = 1 << 30;
    /// 2026-10-15T09:00:00Z, the pinned clock of every case.
    const NOW_MS: i64 = 1_792_054_800_000;

    fn day(text: &str) -> i32 {
        days_since_epoch(text.parse().unwrap())
    }

    /// 100M records in 10 GB (100 bytes each), 300,000 a day, 100 MB of SQLite, 5 GB replay.
    fn sample(oldest: &str, free_bytes: i64) -> Sample {
        Sample {
            taken_at: NOW_MS,
            control_bytes: 50_000_000,
            inbox_bytes: 50_000_000,
            recordings_bytes: 5 * GB,
            analytics_bytes: 10 * GB,
            spill_bytes: 0,
            free_bytes,
            retained_records: 100_000_000,
            ingest_per_day: 300_000,
            oldest_day: Some(day(oldest)),
        }
    }

    #[test]
    fn a_young_installation_has_not_enough_history() {
        let forecast = forecast(&sample("2026-10-05", 60 * GB), 25 * GIB, 4 * GIB);
        assert_eq!(
            forecast.outcome,
            ForecastOutcome::NotEnoughHistory { days: 10 }
        );
        assert_eq!(
            forecast.inputs.full_retention_on.as_deref(),
            Some("2027-10-05")
        );
    }

    #[test]
    fn an_installation_that_fits_levels_off_once_twelve_months_are_retained() {
        // 100 × 300,000 × 365 = 10,950,000,000 + 100,000,000 SQLite + 25 GiB replay cap
        // (26,843,545,600) + 4 GiB reserve (4,294,967,296) = 42,188,512,896, under the
        // 75,100,000,000 bytes of used (15.1 GB) plus free (60 GB) space.
        let forecast = forecast(&sample("2025-11-20", 60 * GB), 25 * GIB, 4 * GIB);
        assert_eq!(
            forecast.outcome,
            ForecastOutcome::LevelsOff {
                size_bytes: 42_188_512_896,
                on: "2026-11-20".into()
            }
        );
        assert_eq!(forecast.inputs.bytes_per_record, 100.0);
        assert_eq!(forecast.inputs.days_until_full_retention, Some(36));
        assert_eq!(forecast.inputs.capacity_bytes, 75_100_000_000);
    }

    #[test]
    fn an_installation_that_does_not_fit_reaches_the_reserve_before_it_levels_off() {
        // Capacity 15.1 + 20 GB = 35,100,000,000 is below the steady 42,188,512,896. Headroom
        // above the reserve: 20,000,000,000 − 4,294,967,296 = 15,705,032,704. Growth to steady
        // state: 42,188,512,896 − 4,294,967,296 − 15,100,000,000 = 22,793,545,600 over 36 days.
        // 15,705,032,704 × 36 / 22,793,545,600 = 24.8, so day 25: 2026-11-09.
        let forecast = forecast(&sample("2025-11-20", 20 * GB), 25 * GIB, 4 * GIB);
        assert_eq!(
            forecast.outcome,
            ForecastOutcome::ReachesReserve {
                on: "2026-11-09".into()
            }
        );
    }

    #[test]
    fn the_median_takes_the_lower_middle_value() {
        assert_eq!(median(vec![]), 0);
        assert_eq!(median(vec![5, 1, 9]), 5);
        assert_eq!(median(vec![4, 1, 9, 7]), 4);
    }
}

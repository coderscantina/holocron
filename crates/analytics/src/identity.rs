//! Identity days, timezone epochs, session policy versions and consent links
//! (0001 Local date, Timezone change, Key retention; 0002 Calculation and policy versions).
//!
//! Collection uses these rules to stamp each record with its identity day and
//! profile link; analytics uses the same versions to derive sessions. Day keys
//! and keyed hashing stay in collection.

use chrono::{Days, Months, NaiveDate, NaiveDateTime, TimeZone};
use chrono_tz::Tz;
use holocron_protocol::retention::Retention;

const MINUTE_MS: i64 = 60_000;
/// Day keys live until the end of their day plus this slack (0001 Key retention).
const KEY_SLACK_MS: i64 = 25 * 60 * MINUTE_MS;
/// Latest and earliest instants identity days are computed for, in Unix milliseconds: the
/// range both engines store, less one day at each end so the next local day still exists.
pub const MAX_TIME_MS: i64 = chrono::DateTime::<chrono::Utc>::MAX_UTC.timestamp_millis() - DAY_MS;
pub const MIN_TIME_MS: i64 = chrono::DateTime::<chrono::Utc>::MIN_UTC.timestamp_millis() + DAY_MS;
const DAY_MS: i64 = 24 * 60 * MINUTE_MS;
/// The 0002 default inactivity window.
pub const DEFAULT_WINDOW_MINUTES: u16 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PolicyError {
    #[error("inactivity window must be 1 to 1440 minutes, got {0}")]
    Window(u16),
    #[error("a new policy version must take effect after the current one")]
    NotAfterCurrent,
    #[error("a timezone change cannot take effect in the past: old day keys are gone")]
    RetroactiveTimezone,
    #[error("the policy is unchanged")]
    Unchanged,
}

/// A site's session settings: the timezone of its identity days and its inactivity window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionPolicy {
    timezone: Tz,
    window_minutes: u16,
}

impl SessionPolicy {
    pub fn new(timezone: Tz, window_minutes: u16) -> Result<Self, PolicyError> {
        if !(1..=1440).contains(&window_minutes) {
            return Err(PolicyError::Window(window_minutes));
        }
        Ok(Self {
            timezone,
            window_minutes,
        })
    }

    pub fn timezone(&self) -> Tz {
        self.timezone
    }

    pub fn window_minutes(&self) -> u16 {
        self.window_minutes
    }

    pub fn window_ms(&self) -> i64 {
        i64::from(self.window_minutes) * MINUTE_MS
    }
}

/// One session policy version. `epoch` counts timezone changes (0001 Timezone
/// change); `version` counts every change (0002 `session_policy_version`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyVersion {
    pub version: i32,
    pub epoch: i32,
    /// First event time this version applies to. Version 1 applies to all earlier times.
    pub effective_ms: i64,
    pub policy: SessionPolicy,
}

/// One local calendar date within one timezone epoch: the lifetime of a daily pseudonym.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IdentityDay {
    pub epoch: i32,
    pub date: NaiveDate,
}

impl IdentityDay {
    /// The `local_day` stored on records: days since 1970-01-01.
    pub fn local_day(&self) -> i32 {
        days_since_epoch(self.date)
    }
}

pub fn days_since_epoch(date: NaiveDate) -> i32 {
    (date - NaiveDate::default()).num_days() as i32
}

pub fn date_of_local_day(local_day: i32) -> NaiveDate {
    NaiveDate::default() + chrono::Duration::days(i64::from(local_day))
}

/// Every session policy version of one site, ascending and never empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SitePolicies {
    versions: Vec<PolicyVersion>,
}

impl SitePolicies {
    /// Version 1, applying to every event time until the first change.
    pub fn new(policy: SessionPolicy) -> Self {
        Self {
            versions: vec![PolicyVersion {
                version: 1,
                epoch: 1,
                effective_ms: i64::MIN,
                policy,
            }],
        }
    }

    /// Versions as stored by their owner, ascending. `None` when there are none.
    pub fn from_versions(versions: Vec<PolicyVersion>) -> Option<Self> {
        (!versions.is_empty()).then_some(Self { versions })
    }

    pub fn versions(&self) -> &[PolicyVersion] {
        &self.versions
    }

    pub fn current(&self) -> &PolicyVersion {
        self.versions.last().expect("never empty")
    }

    /// Appends the next version. A timezone change starts a new epoch and may
    /// not take effect before `now_ms`; a window change may, which makes it an
    /// explicit recalculation from `effective_ms`.
    pub fn change(
        &mut self,
        policy: SessionPolicy,
        effective_ms: i64,
        now_ms: i64,
    ) -> Result<PolicyVersion, PolicyError> {
        let current = *self.current();
        if policy == current.policy {
            return Err(PolicyError::Unchanged);
        }
        if effective_ms <= current.effective_ms {
            return Err(PolicyError::NotAfterCurrent);
        }
        let timezone_changed = policy.timezone != current.policy.timezone;
        if timezone_changed && effective_ms < now_ms {
            return Err(PolicyError::RetroactiveTimezone);
        }
        let next = PolicyVersion {
            version: current.version + 1,
            epoch: current.epoch + i32::from(timezone_changed),
            effective_ms,
            policy,
        };
        self.versions.push(next);
        Ok(next)
    }

    /// The version in effect at `time_ms`.
    pub fn at(&self, time_ms: i64) -> &PolicyVersion {
        let after = self.versions.partition_point(|v| v.effective_ms <= time_ms);
        &self.versions[after.saturating_sub(1)]
    }

    /// The identity day of an event: its local date in the timezone in effect at its time.
    pub fn identity_day(&self, time_ms: i64) -> IdentityDay {
        let version = self.at(time_ms);
        let date = version
            .policy
            .timezone
            .timestamp_millis_opt(time_ms)
            .single()
            .expect("UTC instants map to one local time")
            .date_naive();
        IdentityDay {
            epoch: version.epoch,
            date,
        }
    }

    /// End of the identity day: the next local midnight, or the effective
    /// instant of the next timezone epoch when that comes first.
    pub fn day_end_ms(&self, day: IdentityDay) -> i64 {
        let epoch = self
            .versions
            .iter()
            .find(|v| v.epoch == day.epoch)
            .expect("identity day of a known epoch");
        let next = day.date + Days::new(1);
        let midnight = day_start_ms(epoch.policy.timezone, next);
        match self.versions.iter().find(|v| v.epoch == day.epoch + 1) {
            Some(rotation) => midnight.min(rotation.effective_ms),
            None => midnight,
        }
    }

    /// When the day key of `day` is destroyed. An event for that day received
    /// at or after this instant is keyless (0001 Key retention and keyless events).
    pub fn key_destroyed_at(&self, day: IdentityDay) -> i64 {
        self.day_end_ms(day) + KEY_SLACK_MS
    }

    /// Start of the local date `today - <retention> calendar months` in the timezone in effect
    /// at `now_ms`, clamped to the month's last day. Events before it are outside retention
    /// (0006, 0016). `None` keeps every event.
    pub fn retention_cutoff_ms(&self, now_ms: i64, retention: Retention) -> Option<i64> {
        let tz = self.at(now_ms).policy.timezone;
        let today = tz
            .timestamp_millis_opt(now_ms)
            .single()
            .expect("UTC instants map to one local time")
            .date_naive();
        let cutoff = today
            .checked_sub_months(Months::new(retention.months()?))
            .expect("cutoff within range");
        Some(day_start_ms(tz, cutoff))
    }

    /// [`Self::retention_cutoff_ms`] as the lower bound of a read: the Unix epoch, which no
    /// stored event precedes and local-date arithmetic handles, when every event is kept.
    pub fn retention_floor_ms(&self, now_ms: i64, retention: Retention) -> i64 {
        self.retention_cutoff_ms(now_ms, retention).unwrap_or(0)
    }
}

/// First instant whose local date is `date`. A DST gap at midnight moves it to
/// the first local time that exists.
fn day_start_ms(tz: Tz, date: NaiveDate) -> i64 {
    let mut local: NaiveDateTime = date.and_hms_opt(0, 0, 0).expect("midnight");
    loop {
        if let Some(t) = tz.from_local_datetime(&local).earliest() {
            return t.timestamp_millis();
        }
        local += chrono::Duration::minutes(1);
    }
}

/// One persistent consent interval of a profile (0001 Persistent consent transport).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Consent {
    pub granted_at_ms: i64,
    pub revoked_at_ms: Option<i64>,
}

impl Consent {
    /// An event links to the profile when consent is active at its event time,
    /// whatever its age, so a late event never backfills a profile.
    pub fn links(&self, event_time_ms: i64) -> bool {
        self.granted_at_ms <= event_time_ms && self.revoked_at_ms.is_none_or(|r| event_time_ms < r)
    }
}

/// Identity population of a session (0001 Daily identity of every event).
/// Keyless events have neither identity and form no session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Population {
    /// A daily pseudonym, anonymous or daily identified: one identity day, closes at midnight.
    Daily = 1,
    /// A consented persistent profile: crosses midnight, used by person measures.
    Profile = 2,
}

impl Population {
    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Daily),
            2 => Some(Self::Profile),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(rfc3339: &str) -> i64 {
        chrono::DateTime::parse_from_rfc3339(rfc3339)
            .unwrap()
            .timestamp_millis()
    }

    #[test]
    fn timezone_change_rotates_identity_and_ends_the_old_epoch_day_at_the_change() {
        let mut site =
            SitePolicies::new(SessionPolicy::new(chrono_tz::America::New_York, 30).unwrap());
        let change = ms("2026-09-15T12:00:00Z");
        let v2 = site
            .change(
                SessionPolicy::new(chrono_tz::UTC, 30).unwrap(),
                change,
                change,
            )
            .unwrap();
        assert_eq!((v2.version, v2.epoch), (2, 2));

        let before = site.identity_day(ms("2026-09-15T11:50:00Z"));
        let after = site.identity_day(ms("2026-09-15T12:10:00Z"));
        assert_eq!(before.date, after.date);
        assert_ne!(before, after);
        assert_eq!(site.day_end_ms(before), change);
        assert_eq!(site.key_destroyed_at(before), change + KEY_SLACK_MS);
        assert_eq!(site.day_end_ms(after), ms("2026-09-16T00:00:00Z"));
    }

    #[test]
    fn day_end_follows_dst_day_lengths() {
        let site = SitePolicies::new(SessionPolicy::new(chrono_tz::Europe::Vienna, 30).unwrap());
        let fall = site.identity_day(ms("2025-10-26T22:55:00Z"));
        assert_eq!(fall.date, NaiveDate::from_ymd_opt(2025, 10, 26).unwrap());
        assert_eq!(site.day_end_ms(fall), ms("2025-10-26T23:00:00Z"));
        // fixture key-01: day 2026-06-02 ends 22:00Z, its key is gone from 2026-06-03T23:00Z.
        let day = site.identity_day(ms("2026-06-02T12:05:00Z"));
        assert_eq!(site.key_destroyed_at(day), ms("2026-06-03T23:00:00Z"));
    }

    #[test]
    fn policy_changes_are_ordered_and_timezones_never_change_retroactively() {
        let mut site = SitePolicies::new(SessionPolicy::new(chrono_tz::UTC, 30).unwrap());
        let now = ms("2026-10-01T00:00:00Z");
        let past = now - 86_400_000;
        assert_eq!(
            site.change(
                SessionPolicy::new(chrono_tz::Europe::Vienna, 30).unwrap(),
                past,
                now
            ),
            Err(PolicyError::RetroactiveTimezone)
        );
        let v2 = site
            .change(SessionPolicy::new(chrono_tz::UTC, 15).unwrap(), past, now)
            .unwrap();
        assert_eq!((v2.version, v2.epoch), (2, 1));
        assert_eq!(
            site.change(SessionPolicy::new(chrono_tz::UTC, 45).unwrap(), past, now),
            Err(PolicyError::NotAfterCurrent)
        );
        assert_eq!(
            SessionPolicy::new(chrono_tz::UTC, 0),
            Err(PolicyError::Window(0))
        );
    }

    #[test]
    fn retention_cutoff_clamps_to_month_end() {
        // 0006: from 2028-02-29 the cutoff date is 2027-02-28.
        let utc = SitePolicies::new(SessionPolicy::new(chrono_tz::UTC, 30).unwrap());
        assert_eq!(
            utc.retention_cutoff_ms(ms("2028-02-29T12:00:00Z"), Retention::OneYear),
            Some(ms("2027-02-28T00:00:00Z"))
        );
        // Site A at 2026-11-01T00:00:00Z keeps events from 2025-10-31T23:00:00Z.
        let vienna = SitePolicies::new(SessionPolicy::new(chrono_tz::Europe::Vienna, 30).unwrap());
        assert_eq!(
            vienna.retention_cutoff_ms(ms("2026-11-01T00:00:00Z"), Retention::OneYear),
            Some(ms("2025-10-31T23:00:00Z"))
        );
    }

    #[test]
    fn longer_retention_moves_the_cutoff_by_whole_years() {
        let utc = SitePolicies::new(SessionPolicy::new(chrono_tz::UTC, 30).unwrap());
        let now = ms("2028-02-29T12:00:00Z");
        assert_eq!(
            utc.retention_cutoff_ms(now, Retention::FiveYears),
            Some(ms("2023-02-28T00:00:00Z"))
        );
        assert_eq!(utc.retention_cutoff_ms(now, Retention::Forever), None);
    }

    #[test]
    fn consent_links_by_event_time() {
        let consent = Consent {
            granted_at_ms: 100,
            revoked_at_ms: Some(200),
        };
        assert!(!consent.links(99));
        assert!(consent.links(100));
        assert!(!consent.links(200));
    }
}

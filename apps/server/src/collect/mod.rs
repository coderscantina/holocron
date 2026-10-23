//! The public collection surface: Umami 3.4.0 wire adapters in front of the durable inbox.
//!
//! A request is read under its body cap and the in-flight budget, checked for a collection
//! key, validated, resolved to a site, link or pixel, screened for bots and blocked addresses,
//! given its pseudonym, and admitted. Success is answered only after the inbox commit (0005).
//! Raw IP, user agent, distinct id, key secrets and bodies never leave request memory.

pub(crate) mod client;
pub(crate) mod decimal;
pub(crate) mod geoip;
mod intake;
mod keys;
mod metrics;
mod payload;
pub(crate) mod properties;
mod record;
mod routes;
mod script;
pub(crate) mod status;
pub(crate) mod text;
mod token;
mod wire;

use std::path::Path;
use std::sync::Arc;

use chrono_tz::Tz;
use holocron_analytics::identity::{SessionPolicy, SitePolicies};
use holocron_protocol::operations::collection::{
    CollectionCounter, CollectionOutcome, CollectionRoute, OutcomeReason,
};
use holocron_protocol::retention::Retention;
use holocron_storage::{
    ClientEventId, Event, EventKind, Id, Identify, Outcome, Record, Refusal, Revocation, Storage,
};
use uuid::Uuid;

use crate::clock::Clock;
use crate::config::CollectConfig;
use crate::consents;
use crate::db::Db;
use crate::pseudonyms::{Pseudonyms, Visitor};
use crate::retention;
use crate::sites::policy;
use client::Client;
use geoip::GeoIp;
use intake::Budget;
use metrics::Metrics;
use payload::{Consent, Item, Kind, Source};
use properties::Notes;
use token::{Claims, TokenKey};
use wire::{Answer, BAD_REQUEST, Stored};

pub use routes::router;

/// Events this far ahead of receive time are counted in `event_time_ahead` (0002).
const AHEAD_MS: i64 = 5 * 60 * 1000;
/// Upstream starts a new visit after 30 minutes for untimestamped payloads.
const VISIT_SECONDS: i64 = 1800;

pub struct Collector {
    db: Db,
    storage: Arc<Storage>,
    config: CollectConfig,
    clock: Clock,
    pseudonyms: Pseudonyms,
    tokens: TokenKey,
    bots: isbot::Bots,
    budget: Budget,
    metrics: Metrics,
    tracker: Vec<u8>,
    geoip: Arc<GeoIp>,
}

/// The outcome classification of one answered item, for counters.
type Classified = (CollectionOutcome, Option<OutcomeReason>);

/// What happens to one validated item before admission.
#[expect(
    clippy::large_enum_variant,
    reason = "a short-lived return value; boxing would allocate once per event"
)]
enum Step {
    /// Admit `record`, and `revocation` with it; once committed answer `stored` and count
    /// `accounting`.
    Store {
        record: Record,
        revocation: Option<Record>,
        stored: Stored,
        accounting: Accounting,
    },
    /// Answer without storing.
    Answer(Answer, Classified),
}

/// Diagnostic counters an item adds once it is stored.
#[derive(Debug, Default, Clone, Copy)]
struct Accounting {
    notes: Notes,
    keyless: bool,
    ahead: bool,
    consent_ignored: bool,
}

/// What collection needs to know about a source. Links and pixels use UTC days and never link
/// persistent profiles (0001).
struct SourcePolicy {
    id: Id,
    /// The control storage ID, a hyphenated UUID.
    key: String,
    days: SitePolicies,
    persistent_identity: bool,
    /// Effective event retention (0016).
    retention: Retention,
}

/// The persistent side of one item's identity.
#[derive(Default)]
struct Persistent {
    profile_id: Option<Id>,
    revocation: Option<Record>,
    consent_ignored: bool,
}

impl Collector {
    /// Composes collection, loads the saved outcome counters and the GeoIP database.
    pub async fn new(
        db: Db,
        storage: Arc<Storage>,
        config: CollectConfig,
        clock: Clock,
        root_secret: [u8; 32],
        data_dir: &Path,
    ) -> anyhow::Result<Self> {
        let metrics = Metrics::load(&db, clock.now_ms()).await?;
        let geoip = {
            let (dir, config) = (data_dir.to_owned(), config.clone());
            tokio::task::spawn_blocking(move || GeoIp::open(&dir, &config)).await?
        };
        Ok(Self {
            pseudonyms: Pseudonyms::new(db.clone(), root_secret),
            tokens: TokenKey::derive(&root_secret),
            bots: isbot::Bots::default(),
            budget: Budget::new(config.intake_bytes),
            metrics,
            tracker: script::tracker(config.collect_endpoint.as_deref()),
            geoip: Arc::new(geoip),
            db,
            storage,
            config,
            clock,
        })
    }

    pub fn clock(&self) -> &Clock {
        &self.clock
    }

    pub fn pseudonyms(&self) -> &Pseudonyms {
        &self.pseudonyms
    }

    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    pub fn geoip(&self) -> &Arc<GeoIp> {
        &self.geoip
    }

    /// Outcome and diagnostic counters since [`Metrics::since`].
    pub(crate) fn metrics(&self) -> &Metrics {
        &self.metrics
    }

    /// Saves the outcome counters. A restart keeps what was saved; the background loop saves
    /// every 10 seconds and at a clean stop.
    pub async fn save_counters(&self) -> anyhow::Result<()> {
        self.metrics.save(&self.db).await
    }

    /// The policy of a source, or `None` when it does not exist.
    async fn source(&self, source: Source) -> anyhow::Result<Option<SourcePolicy>> {
        let key = source.id().hyphenated().to_string();
        let found = match source {
            Source::Website(_) => {
                let site: Option<(bool, String)> = sqlx::query_as(
                    "SELECT persistent_identity, coalesce(event_retention,
                         (SELECT event_retention FROM installation_settings WHERE id = 1))
                     FROM sites WHERE id = ?",
                )
                .bind(&key)
                .fetch_optional(&self.db.reader)
                .await?;
                let Some((persistent_identity, code)) = site else {
                    return Ok(None);
                };
                let days = policy::session_policies(&self.db.reader, &key)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("site has no session policy"))?;
                return Ok(Some(SourcePolicy {
                    id: source.id().as_u128(),
                    key,
                    days,
                    persistent_identity,
                    retention: retention::parse(&code)?,
                }));
            }
            Source::Link(_) => {
                retention::effective_live(&self.db.reader, retention::Kind::Link, &key).await?
            }
            Source::Pixel(_) => {
                retention::effective_live(&self.db.reader, retention::Kind::Pixel, &key).await?
            }
        };
        let utc = SessionPolicy::new(Tz::UTC, 30).expect("valid default policy");
        Ok(found.map(|retention| SourcePolicy {
            id: source.id().as_u128(),
            key,
            days: SitePolicies::new(utc),
            persistent_identity: false,
            retention,
        }))
    }

    /// Applies the item's consent signal and finds the persistent profile it links to (0001
    /// Persistent consent transport). `consent` counts only with persistent identity on and an
    /// `id`. A revocation travels to analytics as a record admitted with the item, so a retry
    /// of a refused request sends it again.
    async fn persistent(
        &self,
        item: &Item<'_>,
        source: &SourcePolicy,
        event_time: i64,
        now: i64,
    ) -> anyhow::Result<Persistent> {
        let Some(distinct_id) = item
            .distinct_id
            .as_deref()
            .filter(|_| source.persistent_identity)
        else {
            return Ok(Persistent {
                consent_ignored: item.consent.is_some(),
                ..Persistent::default()
            });
        };
        let profile = self.pseudonyms.profile_id(source.id, distinct_id);
        let mut persistent = Persistent::default();
        match item.consent {
            Some(Consent::Persistent) => {
                consents::grant(&self.db, &source.key, profile, event_time).await?;
            }
            Some(Consent::None) => {
                consents::revoke(&self.db, &source.key, profile, event_time).await?;
                persistent.revocation = Some(Record::Revocation(Revocation {
                    id: Uuid::new_v4().as_u128(),
                    site: source.id,
                    profile_id: Some(profile),
                    revoked_at_ms: event_time,
                    receive_time_ms: now,
                }));
            }
            Some(Consent::Unrecognized) => persistent.consent_ignored = true,
            None => {}
        }
        if consents::active(&self.db, &source.key, profile, event_time).await? {
            persistent.profile_id = Some(profile);
        }
        Ok(persistent)
    }

    /// `key_site` is the site of the request's collection key, if it carries one. The key
    /// vouches for that site only; an item for another source is refused before its lookup, so
    /// the answer says nothing about that source.
    async fn prepare(
        &self,
        item: &Item<'_>,
        client: &Client,
        key_site: Option<Uuid>,
        token: Option<&Claims>,
        now: i64,
    ) -> anyhow::Result<Step> {
        let trusted;
        let client = match key_site {
            None => client,
            Some(site) if item.source != Source::Website(site) => {
                return Ok(Step::Answer(
                    Answer::Unauthorized,
                    (CollectionOutcome::Refused, Some(OutcomeReason::InvalidKey)),
                ));
            }
            Some(_) => {
                let Some(client) =
                    Client::from_payload(item.ip.as_deref(), item.user_agent.as_deref())
                else {
                    return Ok(Step::Answer(
                        Answer::BadRequest(BAD_REQUEST),
                        (CollectionOutcome::Refused, Some(OutcomeReason::Invalid)),
                    ));
                };
                trusted = client;
                &trusted
            }
        };
        let Some(source) = self.source(item.source).await? else {
            let message = match item.source {
                Source::Website(_) => "Website not found.",
                Source::Link(_) => "Link not found.",
                Source::Pixel(_) => "Pixel not found.",
            };
            return Ok(Step::Answer(
                Answer::BadRequest(message),
                (
                    CollectionOutcome::Refused,
                    Some(OutcomeReason::UnknownSource),
                ),
            ));
        };
        if !client.user_agent.is_empty() && self.bots.is_bot(&client.user_agent) {
            return Ok(Step::Answer(
                Answer::Bot,
                (CollectionOutcome::Suppressed, Some(OutcomeReason::Bot)),
            ));
        }
        if client.blocked(&self.config) {
            return Ok(Step::Answer(
                Answer::Forbidden,
                (
                    CollectionOutcome::Suppressed,
                    Some(OutcomeReason::IpBlocked),
                ),
            ));
        }

        let event_time = item.timestamp_ms.unwrap_or(now);
        let website = match item.source {
            Source::Website(id) => Some(id),
            Source::Link(_) | Source::Pixel(_) => None,
        };
        let timestamped = item.timestamp_ms.is_some();
        if source
            .days
            .retention_cutoff_ms(now, source.retention)
            .is_some_and(|cutoff| event_time < cutoff)
        {
            let stored = self.stored(website, None, token, timestamped, now);
            return Ok(Step::Answer(
                Answer::Stored(stored),
                (
                    CollectionOutcome::Suppressed,
                    Some(OutcomeReason::OutsideRetention),
                ),
            ));
        }

        let visitor = match &item.distinct_id {
            Some(distinct_id) => Visitor::Identified { distinct_id },
            None => Visitor::Anonymous {
                ip: &client.ip_text,
                user_agent: &client.user_agent,
            },
        };
        let identity = self
            .pseudonyms
            .derive(source.id, &source.days, event_time, now, visitor)
            .await?;
        let persistent = self.persistent(item, &source, event_time, now).await?;
        let stored = self.stored(website, identity.daily_id, token, timestamped, now);

        let mut notes = Notes::default();
        let properties = match item.data {
            Some(data) if item.kind != Kind::Performance => {
                properties::properties(data, &mut notes)
            }
            _ => Vec::new(),
        };
        let id = Uuid::new_v4().as_u128();
        let record = match item.kind {
            Kind::Identify => Record::Identify(Identify {
                id,
                site: source.id,
                event_time_ms: event_time,
                receive_time_ms: now,
                local_day: identity.local_day,
                daily_id: identity.daily_id,
                profile_id: persistent.profile_id,
                properties,
            }),
            Kind::Event | Kind::Performance => {
                let kind = match (item.kind, item.source, &item.name) {
                    (Kind::Performance, _, _) => EventKind::Performance,
                    (_, Source::Link(_), _) => EventKind::LinkEvent,
                    (_, Source::Pixel(_), _) => EventKind::PixelEvent,
                    (_, _, Some(_)) => EventKind::CustomEvent,
                    (_, _, None) => EventKind::Pageview,
                };
                Record::Event(Event {
                    id,
                    site: source.id,
                    kind,
                    event_time_ms: event_time,
                    receive_time_ms: now,
                    local_day: identity.local_day,
                    daily_id: identity.daily_id,
                    profile_id: persistent.profile_id,
                    client_event_id: item.event_id.as_ref().map(|event_id| ClientEventId {
                        id: event_id.clone(),
                        fingerprint: item.fingerprint,
                    }),
                    dimensions: item.dimensions(&client.user_agent, &client.geo),
                    performance: (kind == EventKind::Performance).then_some(item.performance),
                    revenue: match (kind, item.data) {
                        (EventKind::CustomEvent, Some(data)) => {
                            properties::revenue(data, &mut notes)
                        }
                        _ => None,
                    },
                    properties,
                })
            }
        };
        Ok(Step::Store {
            record,
            revocation: persistent.revocation,
            stored,
            accounting: Accounting {
                notes,
                keyless: identity.daily_id.is_none(),
                ahead: event_time > now + AHEAD_MS,
                consent_ignored: persistent.consent_ignored,
            },
        })
    }

    /// The unchanged `{cache, sessionId, visitId}` body. The session ID is the daily
    /// pseudonym, or a fresh UUID for keyless events, which name no stored session (0005).
    fn stored(
        &self,
        website: Option<Uuid>,
        daily_id: Option<u128>,
        token: Option<&Claims>,
        timestamped: bool,
        now: i64,
    ) -> Stored {
        let session_id = daily_id.map_or_else(Uuid::new_v4, Uuid::from_u128);
        let now_s = now.div_euclid(1000);
        let (visit_id, iat) = match token {
            Some(claims)
                if claims.website_id == website
                    && claims.session_id == session_id
                    && (timestamped || now_s - claims.iat <= VISIT_SECONDS) =>
            {
                (claims.visit_id, claims.iat)
            }
            _ => (Uuid::new_v4(), now_s),
        };
        Stored {
            cache: self
                .tokens
                .sign(&Claims::new(website, session_id, visit_id, iat)),
            session_id,
            visit_id,
        }
    }

    /// Admits the records of one request in one durable commit and counts the result.
    /// `records` holds the items in the order of `accounting`, followed by revocations, which
    /// are not items and are not counted.
    async fn admit(
        &self,
        route: CollectionRoute,
        records: &[Record],
        accounting: &[Accounting],
        now: i64,
    ) -> Result<Vec<Outcome>, Refusal> {
        let result = self.storage.admit(records).await;
        let m = &self.metrics;
        match &result {
            Ok(outcomes) => {
                for (outcome, a) in outcomes.iter().zip(accounting) {
                    match outcome {
                        Outcome::Accepted { .. } => {
                            m.outcome(route, CollectionOutcome::Accepted, None, 1, now);
                            m.count(CollectionCounter::PropertyDropped, a.notes.dropped);
                            m.count(CollectionCounter::PropertyCoerced, a.notes.coerced);
                            m.count(
                                CollectionCounter::RevenueInvalid,
                                a.notes.revenue_invalid.into(),
                            );
                            m.count(CollectionCounter::Keyless, a.keyless.into());
                            m.count(CollectionCounter::EventTimeAhead, a.ahead.into());
                            m.count(CollectionCounter::ConsentIgnored, a.consent_ignored.into());
                        }
                        Outcome::Duplicate { conflict } => {
                            m.outcome(route, CollectionOutcome::Duplicate, None, 1, now);
                            m.count(CollectionCounter::DedupConflict, (*conflict).into());
                        }
                    }
                }
            }
            Err(refusal) => m.outcome(
                route,
                CollectionOutcome::Refused,
                Some(refusal_reason(*refusal)),
                accounting.len() as i64,
                now,
            ),
        }
        result
    }
}

fn refusal_reason(refusal: Refusal) -> OutcomeReason {
    match refusal {
        Refusal::Busy => OutcomeReason::Busy,
        Refusal::Unavailable => OutcomeReason::Unavailable,
    }
}

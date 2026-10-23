//! Services shared by every request, composed once at startup.

use std::ops::Deref;
use std::sync::Arc;

use tokio::sync::Notify;

use holocron_protocol::limits::{LOGIN_ATTEMPTS_PER_MINUTE, LOGIN_CHALLENGE_ATTEMPTS};
use holocron_storage::Storage;

use crate::clock::Clock;
use crate::collect::Collector;
use crate::config::Config;
use crate::db::Db;
use crate::identity::{ChallengeAttempts, RateLimiter};
use crate::maintenance::Maintenance;
use crate::pseudonyms;
use crate::recordings::Recordings;
use crate::sites::policy;
use crate::summaries::Summaries;

#[derive(Clone)]
pub struct AppState(Arc<Inner>);

pub struct Inner {
    pub db: Db,
    pub config: Config,
    pub login_limiter: RateLimiter,
    /// Second-factor attempts per login challenge (decision 0012: 5 wrong codes).
    pub challenge_attempts: ChallengeAttempts,
    pub collector: Collector,
    /// Wakes the correction loop after a command marked identity days dirty.
    pub correction: Notify,
    pub recordings: Recordings,
    /// Maintenance jobs and the shutdown flag readiness reports.
    pub maintenance: Maintenance,
    /// Tells open streams that a site summary changed.
    pub summaries: Summaries,
}

impl AppState {
    /// Composes the services. Creates the installation's root secret on first start and
    /// repairs the analytical copy of every site's session policy.
    pub async fn new(
        db: Db,
        storage: Arc<Storage>,
        config: Config,
        clock: Clock,
    ) -> anyhow::Result<Self> {
        let root = pseudonyms::root_secret(&db, clock.now_ms()).await?;
        policy::mirror_all(&db, &storage).await?;
        let recordings = Recordings::open(
            &config.data_dir,
            &storage,
            config.collect.recording_cap_bytes,
        )
        .await?;
        let collector = Collector::new(
            db.clone(),
            storage,
            config.collect.clone(),
            clock,
            root,
            &config.data_dir,
        )
        .await?;
        Ok(Self(Arc::new(Inner {
            db,
            config,
            login_limiter: RateLimiter::per_minute(LOGIN_ATTEMPTS_PER_MINUTE),
            challenge_attempts: ChallengeAttempts::new(LOGIN_CHALLENGE_ATTEMPTS),
            collector,
            correction: Notify::new(),
            recordings,
            maintenance: Maintenance::default(),
            summaries: Summaries::default(),
        })))
    }
}

impl Deref for AppState {
    type Target = Inner;

    fn deref(&self) -> &Inner {
        &self.0
    }
}

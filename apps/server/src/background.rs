//! Work beside request handling: day key destruction, correction of derived analytics,
//! recording retention and heatmap folding, maintenance jobs, compaction, site summaries, and
//! saving collection counters. Started once the server is composed, stopped before analytical storage closes.

use std::time::Duration;

use holocron_analytics::{Limits, follow};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::state::AppState;

/// How often expired day keys are destroyed (0001: at most an hour after `end(D) + 25h`).
const KEY_DESTRUCTION_INTERVAL: Duration = Duration::from_secs(15 * 60);
/// How often collection outcome counters are saved; `collection.status` documents it.
const COUNTER_SAVE_INTERVAL: Duration = Duration::from_secs(10);

pub struct Background {
    stop: watch::Sender<bool>,
    tasks: Vec<JoinHandle<()>>,
}

impl Background {
    /// Destroys expired day keys now and periodically, and corrects derived analytics whenever
    /// the ingestion watermark moves.
    pub fn start(state: &AppState) -> Self {
        let (stop, stopped) = watch::channel(false);
        let keys = tokio::spawn(destroy_day_keys(state.clone(), stopped.clone()));
        let counters = tokio::spawn(save_counters(state.clone(), stopped.clone()));
        let recordings = tokio::spawn(crate::recordings::maintenance::follow(
            state.clone(),
            stopped.clone(),
        ));
        let jobs = tokio::spawn(crate::maintenance::jobs::follow(
            state.clone(),
            stopped.clone(),
        ));
        let compaction = tokio::spawn(crate::maintenance::compaction::follow(
            state.clone(),
            stopped.clone(),
        ));
        let summaries = tokio::spawn(crate::summaries::follow(state.clone(), stopped.clone()));
        let state = state.clone();
        let correction = tokio::spawn(async move {
            follow(
                state.collector.storage(),
                Limits::default(),
                &state.correction,
                stopped,
            )
            .await;
        });
        Self {
            stop,
            tasks: vec![
                keys, counters, correction, recordings, jobs, compaction, summaries,
            ],
        }
    }

    /// Lets a running correction step and maintenance job step commit, then stops. An
    /// interrupted step would roll back and run again at the next start, so this only saves
    /// work.
    pub async fn stop(self) {
        let _ = self.stop.send(true);
        for task in self.tasks {
            let _ = task.await;
        }
    }
}

async fn destroy_day_keys(state: AppState, mut stop: watch::Receiver<bool>) {
    while !*stop.borrow() {
        let now = state.collector.clock().now_ms();
        if let Err(error) = state.collector.pseudonyms().destroy_expired(now).await {
            tracing::error!(error = %error, "day key destruction failed; retrying later");
        }
        tokio::select! {
            _ = tokio::time::sleep(KEY_DESTRUCTION_INTERVAL) => {}
            _ = stop.changed() => {}
        }
    }
}

/// Saves collection counters periodically and once more when stopped, after the last request.
async fn save_counters(state: AppState, mut stop: watch::Receiver<bool>) {
    loop {
        let stopping = tokio::select! {
            _ = tokio::time::sleep(COUNTER_SAVE_INTERVAL) => false,
            _ = stop.changed() => true,
        };
        if let Err(error) = state.collector.save_counters().await {
            tracing::error!(error = %error, "saving collection counters failed; retrying later");
        }
        if stopping || *stop.borrow() {
            return;
        }
    }
}

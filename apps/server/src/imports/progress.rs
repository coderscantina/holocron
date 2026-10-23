//! Live progress of a run for the setup wizard's stream (decision 0021). Steps report after each
//! committed checkpoint, so what the stream shows is what a resume would continue from. The CLI
//! runs with the default reporter, which does nothing.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use holocron_protocol::setup::{
    SetupImportProgress, SetupImportStep, SetupImportStepName as Name, SetupStepState,
};
use serde_json::Value;
use tokio::sync::watch;

use super::signature::Layout;

/// The estimate follows the rate of this recent window, so a slow start or a fast tail does
/// not distort it for long.
const RATE_WINDOW: Duration = Duration::from_secs(30);

#[derive(Clone, Default)]
pub struct Reporter(Option<Arc<Inner>>);

struct Inner {
    progress: watch::Sender<SetupImportProgress>,
    samples: Mutex<Samples>,
}

/// Rows the running step had read at its recent checkpoints.
#[derive(Default)]
struct Samples {
    step: Option<Name>,
    rows: VecDeque<(Instant, i64)>,
}

/// A step of the importer, the name it checkpoints under and the source table it counts.
const STEPS: [(Name, &str, &str); 6] = [
    (Name::Control, "control", ""),
    (Name::Events, "events", "website_event"),
    (Name::Identify, "identify", "session_data"),
    (Name::Heatmaps, "heatmaps", "heatmap_event"),
    (Name::SessionLinks, "session_links", "session_link"),
    (Name::Replays, "replays", "session_replay"),
];

impl Reporter {
    pub fn new(progress: watch::Sender<SetupImportProgress>) -> Self {
        Self(Some(Arc::new(Inner {
            progress,
            samples: Mutex::default(),
        })))
    }

    /// The steps of `layout` with the row totals of the run's `fingerprint` and the counters of
    /// earlier runs from `steps` ([`super::state::steps`]).
    pub(super) fn start(&self, layout: Layout, fingerprint: &Value, steps: &Value) {
        let Some(inner) = &self.0 else { return };
        let mut list: Vec<SetupImportStep> = STEPS
            .iter()
            .filter(|(name, _, _)| {
                layout == Layout::Umami3Postgresql
                    || matches!(name, Name::Control | Name::Events | Name::Identify)
            })
            .map(|(name, key, table)| SetupImportStep {
                step: *name,
                state: if steps[key]["done"].as_bool() == Some(true) {
                    SetupStepState::Done
                } else {
                    SetupStepState::Pending
                },
                rows: steps[key]["rows"].as_i64().unwrap_or(0),
                total: fingerprint["rows"][table].as_i64().unwrap_or(0),
                eta_seconds: None,
            })
            .collect();
        list.push(SetupImportStep {
            step: Name::Finishing,
            state: SetupStepState::Pending,
            rows: 0,
            total: 0,
            eta_seconds: None,
        });
        if let Some(next) = list.iter_mut().find(|s| s.state == SetupStepState::Pending) {
            next.state = SetupStepState::Running;
        }
        inner.progress.send_modify(|p| p.steps = list);
    }

    /// A committed checkpoint of `step` that read `rows` more source rows.
    pub(super) fn checkpoint(&self, step: &str, rows: i64, done: bool) {
        let Some(inner) = &self.0 else { return };
        let Some((name, _, _)) = STEPS.iter().find(|(_, key, _)| *key == step) else {
            return;
        };
        let now = Instant::now();
        inner.progress.send_modify(|p| {
            let Some(index) = p.steps.iter().position(|s| s.step == *name) else {
                return;
            };
            let current = &mut p.steps[index];
            current.rows += rows;
            if done {
                current.state = SetupStepState::Done;
                current.eta_seconds = None;
                if let Some(next) = p.steps.get_mut(index + 1) {
                    next.state = SetupStepState::Running;
                }
                return;
            }
            current.state = SetupStepState::Running;
            let mut samples = inner.samples.lock().expect("progress samples lock");
            if samples.step != Some(*name) {
                *samples = Samples {
                    step: Some(*name),
                    rows: VecDeque::new(),
                };
            }
            samples.rows.push_back((now, current.rows));
            // The oldest sample kept is the last one before the window, so slow checkpoints
            // still have a rate.
            while samples
                .rows
                .get(1)
                .is_some_and(|(at, _)| now.duration_since(*at) > RATE_WINDOW)
            {
                samples.rows.pop_front();
            }
            current.eta_seconds = samples.rows.front().and_then(|(at, first)| {
                let seconds = now.duration_since(*at).as_secs_f64();
                let rate = (current.rows - first) as f64 / seconds;
                (seconds > 0.0 && rate > 0.0)
                    .then(|| ((current.total - current.rows).max(0) as f64 / rate).ceil() as i64)
            });
        });
    }

    /// Every step with rows is done; correction and reconciliation run.
    pub(super) fn finishing(&self) {
        let Some(inner) = &self.0 else { return };
        inner.progress.send_modify(|p| {
            for step in &mut p.steps {
                step.state = match step.step {
                    Name::Finishing => SetupStepState::Running,
                    _ => SetupStepState::Done,
                };
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn steps_count_rows_against_the_fingerprint_and_resume_from_earlier_runs() {
        let (sender, receiver) = watch::channel(SetupImportProgress::default());
        let reporter = Reporter::new(sender);
        let fingerprint = json!({ "rows": { "website_event": 100, "session_data": 4 } });
        let steps = json!({ "control": { "rows": 9, "done": true }, "events": { "rows": 40, "done": false } });
        reporter.start(Layout::Umami2Mysql, &fingerprint, &steps);
        let names: Vec<_> = receiver.borrow().steps.iter().map(|s| s.step).collect();
        assert_eq!(
            names,
            [Name::Control, Name::Events, Name::Identify, Name::Finishing]
        );

        reporter.checkpoint("events", 20, false);
        std::thread::sleep(Duration::from_millis(20));
        reporter.checkpoint("events", 20, false);
        let events = receiver.borrow().steps[1].clone();
        assert_eq!(
            (events.state, events.rows, events.total),
            (SetupStepState::Running, 80, 100)
        );
        // 20 rows in about 20 ms leaves 20 rows for about as long.
        assert!(events.eta_seconds.is_some_and(|s| (0..=1).contains(&s)));

        reporter.checkpoint("events", 0, true);
        let progress = receiver.borrow().clone();
        assert_eq!(progress.steps[1].state, SetupStepState::Done);
        assert_eq!(progress.steps[2].state, SetupStepState::Running);
    }
}

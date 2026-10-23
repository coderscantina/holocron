//! The application clock (decision 0006). Domain code reads wall time only through it, so
//! tests can pin receive time, retention cutoffs and day-key expiry.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

#[derive(Clone)]
pub struct Clock(Option<Arc<AtomicI64>>);

impl Clock {
    pub fn system() -> Self {
        Self(None)
    }

    /// A clock that stays at `start_ms` until [`Clock::set`] moves it.
    pub fn frozen(start_ms: i64) -> Self {
        Self(Some(Arc::new(AtomicI64::new(start_ms))))
    }

    /// Unix milliseconds.
    pub fn now_ms(&self) -> i64 {
        match &self.0 {
            Some(frozen) => frozen.load(Ordering::SeqCst),
            None => crate::db::now_ms(),
        }
    }

    /// Moves a frozen clock forward. A real clock never goes back, so neither does this one.
    pub fn set(&self, ms: i64) {
        let frozen = self.0.as_ref().expect("only a frozen clock can be set");
        frozen.fetch_max(ms, Ordering::SeqCst);
    }

    pub fn is_test(&self) -> bool {
        self.0.is_some()
    }
}

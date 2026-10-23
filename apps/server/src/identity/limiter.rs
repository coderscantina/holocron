//! Fixed-window attempt counter per client address, for commands that take a password, and the
//! attempt budget of each login challenge.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use holocron_protocol::errors::ErrorCode;

use crate::error::ApiError;

const WINDOW: Duration = Duration::from_secs(60);
/// Above this many tracked addresses, expired windows are dropped before counting.
const PRUNE_ABOVE: usize = 10_000;

pub struct RateLimiter {
    limit: u32,
    windows: Mutex<HashMap<String, (Instant, u32)>>,
}

impl RateLimiter {
    pub fn per_minute(limit: u32) -> Self {
        Self {
            limit,
            windows: Mutex::new(HashMap::new()),
        }
    }

    /// Counts one attempt and refuses it with `rate_limited` past the limit.
    pub fn check(&self, address: &str) -> Result<(), ApiError> {
        let now = Instant::now();
        let mut windows = self
            .windows
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if windows.len() > PRUNE_ABOVE {
            windows.retain(|_, (start, _)| now.duration_since(*start) < WINDOW);
        }
        let window = windows.entry(address.to_owned()).or_insert((now, 0));
        if now.duration_since(window.0) >= WINDOW {
            *window = (now, 0);
        }
        window.1 += 1;
        if window.1 > self.limit {
            return Err(ApiError::new(ErrorCode::RateLimited));
        }
        Ok(())
    }
}

/// Second-factor attempts per login challenge, counted in memory before the code is checked.
/// A wrong code fails its command, which rolls back, so a count kept only in the challenge row
/// lands after the rollback and parallel guesses could pass the limit. The row's `attempts`
/// still carries the count across restarts.
pub struct ChallengeAttempts {
    limit: u32,
    taken: Mutex<HashMap<String, (Instant, u32)>>,
}

impl ChallengeAttempts {
    pub fn new(limit: u32) -> Self {
        Self {
            limit,
            taken: Mutex::new(HashMap::new()),
        }
    }

    /// Takes one attempt of `challenge`, of which the challenge row has recorded `stored`.
    /// False once the limit is used up.
    pub fn take(&self, challenge: &str, stored: u32, lifetime: Duration) -> bool {
        let now = Instant::now();
        let mut taken = self
            .taken
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if taken.len() > PRUNE_ABOVE {
            taken.retain(|_, (start, _)| now.duration_since(*start) < lifetime);
        }
        let entry = taken.entry(challenge.to_owned()).or_insert((now, 0));
        let used = entry.1.max(stored);
        if used >= self.limit {
            return false;
        }
        entry.1 = used + 1;
        true
    }

    /// Drops a challenge that ended with a session.
    pub fn forget(&self, challenge: &str) {
        self.taken
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(challenge);
    }
}

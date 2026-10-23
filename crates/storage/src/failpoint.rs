//! Deterministic process-kill failpoints around each transfer boundary
//! (performance validation, "Handoff, corrections, and failure checks").
//!
//! With the `failpoints` feature, `HOLOCRON_FAILPOINT=<name>:<n>` sends SIGKILL
//! to the process on the n-th hit of `<name>`. Nothing runs after the kill:
//! no destructors, no flush, no checkpoint. Without the feature every call
//! compiles to nothing.

/// Names of every boundary, in protocol order. The last one is inside every
/// derived-data write (correction steps, jobs), after its work and before commit.
pub const BOUNDARIES: [&str; 9] = [
    "admission.before_commit",
    "admission.after_commit",
    "transfer.before_assign",
    "transfer.after_assign",
    "transfer.during_apply",
    "transfer.before_notify",
    "transfer.before_retire",
    "transfer.during_retire",
    "write.before_commit",
];

#[cfg(feature = "failpoints")]
pub(crate) fn hit(name: &'static str) {
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicU64, Ordering};

    static ARMED: OnceLock<Option<(String, u64)>> = OnceLock::new();
    static HITS: AtomicU64 = AtomicU64::new(0);

    let armed = ARMED.get_or_init(|| {
        let spec = std::env::var("HOLOCRON_FAILPOINT").ok()?;
        let (point, n) = spec.rsplit_once(':')?;
        assert!(BOUNDARIES.contains(&point), "unknown failpoint {point}");
        Some((point.to_owned(), n.parse().expect("failpoint count")))
    });
    if let Some((point, n)) = armed
        && point == name
        && HITS.fetch_add(1, Ordering::SeqCst) + 1 == *n
    {
        // SAFETY: plain syscalls on our own pid. Delivery is asynchronous, so
        // this thread parks until the kernel ends the process.
        unsafe {
            libc::kill(libc::getpid(), libc::SIGKILL);
            loop {
                libc::pause();
            }
        }
    }
}

#[cfg(not(feature = "failpoints"))]
#[inline(always)]
pub(crate) fn hit(_name: &'static str) {}

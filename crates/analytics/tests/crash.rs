//! Process kill inside correction transactions, 20 fixed seeds.
//! Run with `cargo test -p holocron-analytics --features failpoints`.
#![cfg(feature = "failpoints")]

use std::os::unix::process::ExitStatusExt;
use std::process::Command;

const CHILD: &str = env!("CARGO_BIN_EXE_analytics-crash-child");
const SIGKILL: i32 = 9;

#[test]
fn crash_during_correction_leaves_old_or_complete_steps() {
    for seed in 0..20u64 {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().to_str().expect("utf-8 path");
        // Hit 1 kills the first derivation; hits 2 to 5 kill steps replacing rows.
        let hit = 1 + seed % 5;
        let run = Command::new(CHILD)
            .args(["run", path, &seed.to_string()])
            .env("HOLOCRON_FAILPOINT", format!("write.before_commit:{hit}"))
            .output()
            .expect("spawn run");
        assert_eq!(
            run.status.signal(),
            Some(SIGKILL),
            "seed {seed}: child was not killed: {:?} {}",
            run.status,
            String::from_utf8_lossy(&run.stderr)
        );
        let verify = Command::new(CHILD)
            .args(["verify", path, &seed.to_string()])
            .output()
            .expect("spawn verify");
        assert!(
            verify.status.success(),
            "seed {seed}: {}",
            String::from_utf8_lossy(&verify.stderr)
        );
        println!(
            "seed {seed} hit {hit}: {}",
            String::from_utf8_lossy(&verify.stdout).trim()
        );
    }
}

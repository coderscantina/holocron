//! Process-kill recovery at every transfer boundary, 20 fixed seeds each.
//! Run with `cargo test -p holocron-storage --features failpoints --release`.
#![cfg(feature = "failpoints")]

use std::os::unix::process::ExitStatusExt;
use std::process::Command;

const SEEDS_PER_BOUNDARY: u64 = 20;
const CHILD: &str = env!("CARGO_BIN_EXE_storage-crash-child");
const SIGKILL: i32 = 9;

fn kill_and_recover(boundary: &str) {
    for seed in 0..SEEDS_PER_BOUNDARY {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().to_str().expect("utf-8 path");
        // Hit counts 1..=4 move the kill across different batches and requests.
        let hit = 1 + seed % 4;
        let run = Command::new(CHILD)
            .args(["run", path, &seed.to_string()])
            .env("HOLOCRON_FAILPOINT", format!("{boundary}:{hit}"))
            .output()
            .expect("spawn run");
        assert_eq!(
            run.status.signal(),
            Some(SIGKILL),
            "{boundary} seed {seed}: child was not killed: {:?} {}",
            run.status,
            String::from_utf8_lossy(&run.stderr)
        );
        let verify = Command::new(CHILD)
            .args(["verify", path, &seed.to_string(), boundary])
            .output()
            .expect("spawn verify");
        assert!(
            verify.status.success(),
            "{boundary} seed {seed}: {}",
            String::from_utf8_lossy(&verify.stderr)
        );
        println!(
            "{boundary} seed {seed} hit {hit}: {}",
            String::from_utf8_lossy(&verify.stdout).trim()
        );
    }
}

#[test]
fn admission_before_commit() {
    kill_and_recover("admission.before_commit");
}

#[test]
fn admission_after_commit_before_response() {
    kill_and_recover("admission.after_commit");
}

#[test]
fn after_acknowledgement_before_assignment() {
    kill_and_recover("transfer.before_assign");
}

#[test]
fn after_stable_assignment() {
    kill_and_recover("transfer.after_assign");
}

#[test]
fn during_duckdb_writes_before_commit() {
    kill_and_recover("transfer.during_apply");
}

#[test]
fn after_duckdb_commit_before_notification() {
    kill_and_recover("transfer.before_notify");
}

#[test]
fn after_notification_before_retirement() {
    kill_and_recover("transfer.before_retire");
}

#[test]
fn during_sqlite_retirement() {
    kill_and_recover("transfer.during_retire");
}

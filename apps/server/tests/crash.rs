//! Process-kill recovery of the real `holocron` binary under concurrent collection load.
//! Each round starts the server, sends events from several clients, and SIGKILLs it at a
//! seeded moment. Afterwards every acknowledged event must be stored exactly once, and no
//! event may be stored twice.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use holocron_storage::{Budgets, Lane, Storage};

const BIN: &str = env!("CARGO_BIN_EXE_holocron");
const ROUNDS: u64 = 20;
const CLIENTS: usize = 4;
const SITE: &str = "5e1a0000-0000-4000-8000-0000000000c1";
/// The development disk may hold less than the 4 GiB production reserve.
const RESERVE_MIB: &str = "64";

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn serve(data: &Path, port: u16) -> Child {
    let child = Command::new(BIN)
        .args(["serve", "--data-dir"])
        .arg(data)
        .args([
            "--bind",
            &format!("127.0.0.1:{port}"),
            "--disk-reserve-mib",
            RESERVE_MIB,
        ])
        .env("HOLOCRON_LOG", "error")
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while post(port, "GET", "/health", "").is_none() {
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    child
}

/// One HTTP/1.1 request on a fresh connection; the status code, or `None` when the
/// connection failed or the answer was cut off.
fn post(port: u16, method: &str, path: &str, body: &str) -> Option<u16> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .ok()?;
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: crash.test\r\nUser-Agent: Mozilla/5.0 (X11; Linux x86_64) Chrome/140.0.0.0 Safari/537.36\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).ok()?;
    let mut answer = String::new();
    stream.read_to_string(&mut answer).ok()?;
    let status: u16 = answer.split(' ').nth(1)?.parse().ok()?;
    // A complete answer ends with its JSON body.
    (status != 200 || answer.ends_with('}')).then_some(status)
}

fn event(path: &str) -> String {
    format!(
        r#"{{"type":"event","payload":{{"website":"{SITE}","hostname":"crash.test","url":"{path}"}}}}"#
    )
}

#[test]
fn every_acknowledged_event_survives_sigkill_exactly_once() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let password = dir.path().join("password");
    std::fs::write(&password, "correct horse battery").unwrap();
    let init = Command::new(BIN)
        .args(["init", "--admin-email", "admin@crash.test", "--data-dir"])
        .arg(&data)
        .arg("--admin-password-file")
        .arg(&password)
        .output()
        .unwrap();
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let db = holocron_server::db::Db::open(&data).await.unwrap();
        let workspace: String = sqlx::query_scalar("SELECT id FROM workspaces")
            .fetch_one(&db.reader)
            .await
            .unwrap();
        sqlx::query("INSERT INTO sites (id, workspace_id, name, domain, created_at) VALUES (?, ?, 'crash', 'crash.test', 0)")
            .bind(SITE)
            .bind(&workspace)
            .execute(&db.writer)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO session_policies (site_id, version, epoch, effective_at, timezone, window_minutes, created_at)
             VALUES (?, 1, 1, -9223372036854775808, 'Europe/Vienna', 30, 0)",
        )
        .bind(SITE)
        .execute(&db.writer)
        .await
        .unwrap();
        db.close().await;
    });

    let attempted = Arc::new(Mutex::new(BTreeSet::new()));
    let acked = Arc::new(Mutex::new(BTreeSet::new()));
    for round in 0..ROUNDS {
        let port = free_port();
        let mut server = serve(&data, port);
        let clients: Vec<_> = (0..CLIENTS)
            .map(|client| {
                let (attempted, acked) = (attempted.clone(), acked.clone());
                std::thread::spawn(move || {
                    for n in 0.. {
                        let path = format!("/crash/{round}/{client}/{n}");
                        attempted.lock().unwrap().insert(path.clone());
                        match post(port, "POST", "/api/send", &event(&path)) {
                            Some(200) => {
                                acked.lock().unwrap().insert(path);
                            }
                            Some(status) => panic!("{path}: unexpected status {status}"),
                            None => return,
                        }
                    }
                })
            })
            .collect();
        // Seeded kill moment between 50 and 430 ms into the round.
        std::thread::sleep(Duration::from_millis(50 + (round * 7919) % 380));
        server.kill().unwrap();
        server.wait().unwrap();
        for client in clients {
            client.join().unwrap();
        }
    }

    // Final recovery: open the data directory directly and drain every pending batch.
    let stored: BTreeMap<String, i64> = runtime.block_on(async {
        let storage = Storage::open(
            &data,
            Budgets {
                disk_reserve_bytes: 0,
                ..Budgets::default()
            },
        )
        .await
        .unwrap();
        storage.drain().await;
        let rows = storage
            .read(Lane::Report, |s| {
                let mut statement = s
                    .connection()
                    .prepare("SELECT path, count(*) FROM events GROUP BY path")?;
                let rows = statement
                    .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
                    .collect::<Result<BTreeMap<_, _>, _>>()?;
                Ok(rows)
            })
            .await
            .unwrap()
            .value;
        storage.close().await.unwrap();
        rows
    });

    let attempted = attempted.lock().unwrap();
    let acked = acked.lock().unwrap();
    let lost: Vec<_> = acked.iter().filter(|p| !stored.contains_key(*p)).collect();
    let doubled: Vec<_> = stored.iter().filter(|(_, n)| **n != 1).collect();
    let unknown: Vec<_> = stored.keys().filter(|p| !attempted.contains(*p)).collect();
    assert!(lost.is_empty(), "acknowledged but lost: {lost:?}");
    assert!(doubled.is_empty(), "stored more than once: {doubled:?}");
    assert!(unknown.is_empty(), "stored but never sent: {unknown:?}");
    println!(
        "rounds={ROUNDS} clients={CLIENTS} attempted={} acked={} stored={} committed_unacked={}",
        attempted.len(),
        acked.len(),
        stored.len(),
        stored.len() - acked.len()
    );
}
